//! Human-readable array formatting, J session style: numeric columns
//! aligned, higher-rank arrays printed as planes separated by blank lines.

use crate::array::{Array, Data};
use crate::dtype::DType;

/// How a boxed array is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxStyle {
    /// J: a table of cells fenced with `+`, `-` and `|`.
    Fenced,
    /// APL: the cells side by side, spaced by `nested_gap` at rank 1
    /// (matching GNU APL) or uniformly by one space at rank 2 and above;
    /// see docs/coverage.md.
    Spaced,
}

/// Display conventions that differ between languages.
#[derive(Clone, Copy, Debug)]
pub struct FmtOpts {
    /// Negative-number prefix: `_` for J, `¯` for APL.
    pub neg: char,
    /// Separator between the parts of a complex number: `j` for J, `J` for
    /// APL.
    pub imag: char,
    pub boxes: BoxStyle,
    /// Whether a character is a BYTE, which is what J's literal type holds.
    /// The formatted text then carries one `char` per byte, each holding
    /// that byte's value: [`format_raw`] hands it over as it stands, which
    /// is what `":` needs, and [`format_array`] writes the bytes out and
    /// reads them back as UTF-8, which is what a session shows. A display
    /// width is counted in characters either way, so a boxed literal is
    /// fenced to what the text occupies rather than to what it weighs.
    ///
    /// False for APL, whose characters are Unicode, and for J under the
    /// `j_unicode_strings` extension.
    pub bytes: bool,
}

impl FmtOpts {
    pub const J: FmtOpts =
        FmtOpts { neg: '_', imag: 'j', boxes: BoxStyle::Fenced, bytes: true };
    pub const APL: FmtOpts =
        FmtOpts { neg: '¯', imag: 'J', boxes: BoxStyle::Spaced, bytes: false };

    /// J's conventions under a set of rules: the extensions decide whether
    /// a character is a byte.
    pub fn j(rules: crate::frontend::Rules) -> FmtOpts {
        FmtOpts {
            bytes: !rules.extensions.has(crate::extensions::Extensions::J_UNICODE_STRINGS),
            ..FmtOpts::J
        }
    }
}

/// Significant digits kept when displaying a float.
const SIG_DIGITS: usize = 6;

/// Format an array as a session shows it. No trailing newline.
///
/// Where a character is a byte (J), the text [`format_raw`] builds holds one
/// `char` per byte; this writes those bytes out and reads the result back as
/// UTF-8, which is exactly what a J session does with a literal. A byte that
/// is not part of any character survives as the replacement character, as it
/// does when a terminal is handed one.
pub fn format_array(a: &Array, opts: &FmtOpts) -> String {
    let text = format_raw(a, opts);
    if !opts.bytes {
        return text;
    }
    String::from_utf8_lossy(&as_bytes(text.chars())).into_owned()
}

/// The text a character vector spells.
///
/// Where a character is a byte (J), the bytes are read back as UTF-8, which
/// is what turns a literal back into the source text it was written as —
/// what `".` compiles and what a quoted definition body is lexed from.
/// Anywhere else, the characters are the text already.
pub fn text_of<I: IntoIterator<Item = char>>(chars: I, bytes: bool) -> String {
    if !bytes {
        return chars.into_iter().collect();
    }
    String::from_utf8_lossy(&as_bytes(chars)).into_owned()
}

/// The bytes a character vector writes: one byte per character where it
/// holds one, and the UTF-8 spelling of anything wider — which is what a
/// character too wide for a byte can only have come from (`u:`).
fn as_bytes<I: IntoIterator<Item = char>>(chars: I) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4];
    for c in chars {
        match u8::try_from(c as u32) {
            Ok(b) => out.push(b),
            Err(_) => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
        }
    }
    out
}

/// Format an array in the units it holds: one character per element, so a
/// byte-oriented literal gives one `char` per byte. This is what `":` and
/// `⍕` take, since the display they answer with is itself an array of the
/// argument's own characters.
pub fn format_raw(a: &Array, opts: &FmtOpts) -> String {
    // A sparse array shows what it stores, not what it stands for.
    if let Some(s) = a.sparse_parts() {
        return format_sparse(a, s, opts);
    }
    // An array with an empty axis has nothing to show.
    if a.shape.contains(&0) {
        return String::new();
    }
    // The planes are laid out by reading the buffer in order, so a
    // column-major one is materialised first. Printing already costs more
    // than the copy does.
    if !a.is_row_major() {
        return format_raw(&a.to_row_major(), opts);
    }
    if a.dtype() == DType::Box {
        // A boxed array whose every element is a simple scalar (peeling
        // through any number of `⊂` layers) is APL's MIXED SIMPLE array:
        // depth 1, and drawn the way a plain array is rather than with a
        // nested display's extra spacing.
        match mixed_simple_texts(a, opts) {
            Some(texts) if opts.boxes == BoxStyle::Spaced && a.rank() == 1 => {
                return mixed_vector_line(a, &texts)
            }
            Some(texts) if opts.boxes == BoxStyle::Spaced => {
                return laid_out(&a.shape, texts, Cells::Right, opts)
            }
            // A mixed VECTOR with at least one non-scalar item: GNU's
            // nested display, not the fenced/uniformly-spaced box drawing.
            None if opts.boxes == BoxStyle::Spaced && a.rank() == 1 => {
                return nested_vector_line(a, opts)
            }
            _ => return format_boxed(a, opts),
        }
    }
    let texts: Vec<String> = (0..a.count()).map(|i| format_atom(&a.data, i, opts)).collect();
    laid_out(&a.shape, texts, Cells::of(a.dtype()), opts)
}

/// A sparse array: one line per stored entry, that entry's position along
/// the sparse axes, then `|`, then the cell it holds. Positions and values
/// each align in their own column, over the whole display. An array with
/// nothing stored — every position the sparse element — shows nothing, as
/// an empty dense one does.
fn format_sparse(a: &Array, s: &crate::sparse::Sparse, opts: &FmtOpts) -> String {
    if s.entries == 0 {
        return String::new();
    }
    let k = s.axes.len();
    let width = s.cell_size(&a.shape);
    let index: Vec<String> = s.indices.iter().map(|&i| format_i64(i as i64, opts)).collect();
    let value: Vec<String> =
        (0..s.entries * width).map(|i| format_atom(&a.data, i, opts)).collect();
    let index_widths = column_widths(&index, k.max(1), opts);
    let value_widths = column_widths(&value, width.max(1), opts);
    let mut out = String::new();
    for e in 0..s.entries {
        if e > 0 {
            out.push('\n');
        }
        push_row(&mut out, &index[e * k..(e + 1) * k], &index_widths, Cells::Right, opts);
        out.push_str(" | ");
        push_row(&mut out, &value[e * width..(e + 1) * width], &value_widths, Cells::Right, opts);
    }
    out
}

/// How the formatted elements of one row sit next to each other.
#[derive(Clone, Copy, PartialEq)]
enum Cells {
    /// Numbers: one space between columns, each column right-aligned.
    Right,
    /// Characters: no separator at all, because the row IS the text.
    Text,
    /// Symbols: one space between columns, each column left-aligned and
    /// padded on the right, which is how J prints a table of names.
    Left,
}

impl Cells {
    fn of(dtype: DType) -> Cells {
        match dtype {
            DType::Char => Cells::Text,
            DType::Symbol => Cells::Left,
            _ => Cells::Right,
        }
    }
}

/// Peel through scalar box wrappings (`⊂x`, `⊂⊂x`, and so on) to the value
/// they finally hold, and how many layers were peeled. A box holding
/// something that is itself non-scalar, or a value that was never a box,
/// is its own leaf with zero layers.
fn peel(a: &Array) -> (usize, &Array) {
    let mut n = 0;
    let mut cur = a;
    while cur.rank() == 0 && cur.dtype() == DType::Box {
        cur = &cur.as_boxes().expect("boxed scalar")[0];
        n += 1;
    }
    (n, cur)
}

/// The scalar each element of a boxed array holds, where every one of them
/// peels down to a simple scalar and nothing else — `⊂⊂5` counts, since
/// enclosing a scalar again and again never makes it non-scalar.
fn mixed_simple_texts(a: &Array, opts: &FmtOpts) -> Option<Vec<String>> {
    let boxes = a.as_boxes()?;
    let mut texts = Vec::with_capacity(boxes.len());
    for b in boxes {
        let (_, leaf) = peel(b);
        if leaf.rank() != 0 {
            return None;
        }
        texts.push(format_atom(&leaf.data, 0, opts));
    }
    Some(texts)
}

/// A mixed simple VECTOR on one line. A run of characters beside each
/// other is text and runs together; every other join takes one space, so
/// `1 2,'ab'` shows as `1 2 ab`. Higher ranks align in columns instead,
/// and there each character is a column of its own.
fn mixed_vector_line(a: &Array, texts: &[String]) -> String {
    let letters: Vec<bool> = match a.as_boxes() {
        Some(items) => items.iter().map(|e| peel(e).1.dtype() == DType::Char).collect(),
        None => vec![false; texts.len()],
    };
    let mut out = String::new();
    for (i, t) in texts.iter().enumerate() {
        if i > 0 && !(letters[i] && letters[i - 1]) {
            out.push(' ');
        }
        out.push_str(t);
    }
    out
}

/// How much a leaf widens the gap next to it in a nested display: nothing
/// for a scalar, one column per axis for a numeric or boxed structure, and
/// one column fewer for a character array, because a row of characters
/// already reads as text on its own — a character VECTOR costs nothing
/// extra (same as a scalar), a character MATRIX costs one column.
fn gap_extra(leaf: &Array) -> usize {
    match leaf.rank() {
        0 => 0,
        r if leaf.dtype() == DType::Char => r - 1,
        r => r,
    }
}

/// The gap between two adjacent items of a nested vector: no separator at
/// all when both are lone characters (a run of text), otherwise one space
/// plus however much the more complex neighbour's own shape asks for.
fn nested_gap(left: &Array, right: &Array) -> usize {
    let text_run = left.rank() == 0
        && right.rank() == 0
        && left.dtype() == DType::Char
        && right.dtype() == DType::Char;
    if text_run { 0 } else { 1 + gap_extra(left).max(gap_extra(right)) }
}

/// A boxed VECTOR with at least one non-scalar item: GNU's general nested
/// display. Each item's OWN content draws plain, with no box wrapping of
/// its own — every space around it comes from here instead. The gap
/// between two items is `nested_gap`; the vector's own margin, front and
/// back, is set by how many `⊂` layers wrap its first and its last item
/// respectively (never fewer than one).
fn nested_vector_line(a: &Array, opts: &FmtOpts) -> String {
    let items = a.as_boxes().expect("boxed vector");
    let peeled: Vec<(usize, &Array)> = items.iter().map(peel).collect();
    let cells: Vec<(Vec<String>, usize)> = peeled.iter().map(|(_, leaf)| block(leaf, opts)).collect();
    let height = cells.iter().map(|(lines, _)| lines.len()).max().unwrap_or(1);
    let lead = " ".repeat(peeled.first().map_or(1, |(n, _)| (*n).max(1)));
    let trail = " ".repeat(peeled.last().map_or(1, |(n, _)| (*n).max(1)));
    let mut rows = vec![String::new(); height];
    for i in 0..peeled.len() {
        if i > 0 {
            let sep = " ".repeat(nested_gap(peeled[i - 1].1, peeled[i].1));
            for row in &mut rows {
                row.push_str(&sep);
            }
        }
        let (lines, w) = &cells[i];
        for (r, row) in rows.iter_mut().enumerate() {
            let text = lines.get(r).map(String::as_str).unwrap_or("");
            row.push_str(text);
            for _ in 0..w.saturating_sub(width(text, opts)) {
                row.push(' ');
            }
        }
    }
    rows.iter().map(|r| format!("{lead}{r}{trail}")).collect::<Vec<_>>().join("\n")
}

/// One formatted element per position, laid out for the shape: a vector on
/// one line, higher ranks as aligned columns and planes.
fn laid_out(shape: &[usize], texts: Vec<String>, cells: Cells, opts: &FmtOpts) -> String {
    let rank = shape.len();
    let a_shape = shape;
    match rank {
        0 => texts.into_iter().next().unwrap_or_default(),
        1 if cells == Cells::Text => texts.concat(),
        1 => texts.join(" "),
        _ => {
            let ncols = a_shape[rank - 1];
            let nrows = a_shape[rank - 2];
            // Column widths span every plane, so planes stay aligned with
            // each other and not just internally.
            let widths = if cells == Cells::Text {
                vec![0; ncols]
            } else {
                column_widths(&texts, ncols, opts)
            };
            let frame = &a_shape[..rank - 2];
            let plane_size = nrows * ncols;
            let planes: usize = frame.iter().product();
            let mut out = String::new();
            for p in 0..planes {
                if p > 0 {
                    // One newline ends the previous line, the rest are blanks.
                    out.push_str(&"\n".repeat(plane_gap(frame, p) + 1));
                }
                for r in 0..nrows {
                    if r > 0 {
                        out.push('\n');
                    }
                    let start = p * plane_size + r * ncols;
                    push_row(&mut out, &texts[start..start + ncols], &widths, cells, opts);
                }
            }
            out
        }
    }
}

/// A boxed array as its language draws it: the last two axes form a table
/// of cells, each holding its own contents' display, and the axes above
/// them separate planes exactly as they do for numbers.
fn format_boxed(a: &Array, opts: &FmtOpts) -> String {
    let boxes = a.as_boxes().expect("boxed data");
    let blocks: Vec<(Vec<String>, usize)> = boxes.iter().map(|b| block(b, opts)).collect();
    let rank = a.rank();
    let (nrows, ncols) = match rank {
        0 => (1, 1),
        1 => (1, a.shape[0]),
        _ => (a.shape[rank - 2], a.shape[rank - 1]),
    };
    // Column widths span the whole array, as they do for numeric columns.
    let mut widths = vec![0usize; ncols];
    for (i, (_, w)) in blocks.iter().enumerate() {
        widths[i % ncols] = widths[i % ncols].max(*w);
    }
    let frame: &[usize] = if rank > 2 { &a.shape[..rank - 2] } else { &[] };
    let planes: usize = frame.iter().product();
    let plane_size = nrows * ncols;
    let mut out = String::new();
    for p in 0..planes.max(1) {
        if p > 0 {
            out.push_str(&"\n".repeat(plane_gap(frame, p) + 1));
        }
        push_boxed_plane(
            &mut out,
            &blocks[p * plane_size..(p + 1) * plane_size],
            nrows,
            ncols,
            &widths,
            opts,
        );
    }
    out
}

/// One box's contents as display lines, and the width they need.
///
/// An empty array has no text at all, and its SHAPE decides the cell:
/// every axis but the last counts a row, and the last one is how wide the
/// cell draws. So `<''` is one empty line inside a zero-wide cell, `<0 3$0`
/// is a cell three wide with no lines in it, and `<2 0$0` is two empty
/// lines. The width has to travel beside the lines because a cell with no
/// lines still has one.
fn block(a: &Array, opts: &FmtOpts) -> (Vec<String>, usize) {
    if a.count() == 0 && a.rank() > 0 {
        let rank = a.rank();
        let rows: usize = a.shape[..rank - 1].iter().product();
        let w = a.shape[rank - 1];
        return (vec![" ".repeat(w); rows], w);
    }
    let text = format_raw(a, opts);
    if text.is_empty() {
        return (vec![String::new()], 0);
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let w = lines.iter().map(|l| width(l, opts)).max().unwrap_or(0);
    (lines, w)
}

fn push_boxed_plane(
    out: &mut String,
    blocks: &[(Vec<String>, usize)],
    nrows: usize,
    ncols: usize,
    widths: &[usize],
    opts: &FmtOpts,
) {
    let fence = opts.boxes == BoxStyle::Fenced;
    let border: String = if fence {
        let mut s = String::from("+");
        for &w in widths {
            s.push_str(&"-".repeat(w));
            s.push('+');
        }
        s
    } else {
        String::new()
    };
    let mut lines: Vec<String> = Vec::new();
    for r in 0..nrows {
        if fence {
            lines.push(border.clone());
        }
        let row = &blocks[r * ncols..(r + 1) * ncols];
        // A row is as tall as its tallest cell; the others are padded
        // underneath, which is where J puts the blanks.
        let height = row.iter().map(|(lines, _)| lines.len()).max().unwrap_or(1);
        for k in 0..height {
            let mut line = String::new();
            line.push(if fence { '|' } else { ' ' });
            for (c, (cell, _)) in row.iter().enumerate() {
                if !fence && c > 0 {
                    line.push(' ');
                }
                let text = cell.get(k).map(String::as_str).unwrap_or("");
                line.push_str(text);
                for _ in 0..widths[c].saturating_sub(width(text, opts)) {
                    line.push(' ');
                }
                if fence {
                    line.push('|');
                }
            }
            if !fence {
                line.push(' ');
            }
            lines.push(line);
        }
    }
    if fence {
        lines.push(border);
    }
    out.push_str(&lines.join("\n"));
}

/// Blank lines before plane `p`: one for a step along axis -3, two along
/// axis -4, and so on. `frame` is the shape without its last two axes.
fn plane_gap(frame: &[usize], p: usize) -> usize {
    // The step size is one plus the number of trailing odometer digits of
    // `p` that have just rolled over to zero.
    let mut gap = 1;
    let mut rest = p;
    for &n in frame.iter().rev() {
        if rest % n != 0 {
            break;
        }
        rest /= n;
        gap += 1;
    }
    gap
}

/// Widest formatted element per column index, taken over the whole array.
fn column_widths(texts: &[String], ncols: usize, opts: &FmtOpts) -> Vec<usize> {
    let mut widths = vec![0usize; ncols];
    for (i, t) in texts.iter().enumerate() {
        let j = i % ncols;
        widths[j] = widths[j].max(width(t, opts));
    }
    widths
}

fn push_row(out: &mut String, row: &[String], widths: &[usize], cells: Cells, opts: &FmtOpts) {
    for (j, cell) in row.iter().enumerate() {
        if cells == Cells::Text {
            out.push_str(cell);
            continue;
        }
        if j > 0 {
            out.push(' ');
        }
        let pad = widths[j].saturating_sub(width(cell, opts));
        if cells == Cells::Right {
            for _ in 0..pad {
                out.push(' ');
            }
        }
        out.push_str(cell);
        if cells == Cells::Left {
            for _ in 0..pad {
                out.push(' ');
            }
        }
    }
}

/// Display width in characters, which is not the count of what the text
/// holds: the APL minus sign is multi-byte, and where a character is a byte
/// (J) a continuation byte belongs to the character before it and takes no
/// column of its own.
fn width(s: &str, opts: &FmtOpts) -> usize {
    if opts.bytes {
        return s.chars().filter(|&c| !matches!(c as u32, 0x80..=0xBF)).count();
    }
    s.chars().count()
}

fn format_atom(data: &Data, i: usize, opts: &FmtOpts) -> String {
    match data {
        Data::Bool(v) => (if v[i] != 0 { "1" } else { "0" }).to_string(),
        Data::I64(v) => format_i64(v[i], opts),
        Data::Ext(v) => with_neg_sign(&v[i].to_string(), opts),
        Data::Rat(v) => with_neg_sign(&v[i].to_string(), opts),
        Data::F64(v) => format_f64(v[i], opts),
        Data::Complex(v) => format_complex(v[i], opts),
        Data::Char(v) => v[i].to_string(),
        // A symbol prints as its name behind the backtick that makes one.
        Data::Symbol(v) => format!("`{}", crate::symbol::name(v[i])),
        // Boxed data takes the drawing path before reaching here.
        Data::Box(_) => String::new(),
    }
}

/// A complex number, as both references print one: the two parts joined by
/// `j`/`J`, and the real part alone when the imaginary part is exactly zero.
/// The demotion is in the display only — the value keeps its complex type,
/// which is what `3!:0` reports of it in J.
fn format_complex(z: crate::complex::Cx, opts: &FmtOpts) -> String {
    if z[1] == 0.0 {
        return format_f64(z[0], opts);
    }
    format!("{}{}{}", format_f64(z[0], opts), opts.imag, format_f64(z[1], opts))
}

fn format_i64(v: i64, opts: &FmtOpts) -> String {
    with_neg_sign(&v.to_string(), opts)
}

/// A Rust-formatted number with its leading `-` replaced by the language’s
/// own negative sign. An extended integer and a rational both arrive here
/// already spelled the way J spells them (`123`, `_1r2` once the sign is
/// swapped), so nothing else has to be rewritten.
fn with_neg_sign(s: &str, opts: &FmtOpts) -> String {
    match s.strip_prefix('-') {
        Some(rest) => with_sign(rest, opts),
        None => s.to_string(),
    }
}

fn format_f64(x: f64, opts: &FmtOpts) -> String {
    if x.is_nan() {
        return format!("{}.", opts.neg);
    }
    if x.is_infinite() {
        // J spells the infinities `_` and `__`; APL has no standard glyph.
        return match (opts.neg, x > 0.0) {
            ('_', true) => "_".to_string(),
            ('_', false) => "__".to_string(),
            (_, true) => "∞".to_string(),
            (neg, false) => format!("{neg}∞"),
        };
    }
    let magnitude = x.abs();
    // Round to `SIG_DIGITS` first, then decide how to spell the result;
    // scientific formatting hands us the digits and the exponent directly.
    let sci = format!("{:.*e}", SIG_DIGITS - 1, magnitude);
    let (mantissa, exponent) = sci.split_once('e').expect("scientific form has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().expect("exponent is an integer");
    let body = if exponent >= 12 || exponent <= -6 {
        let mut s = trim_fraction(&place_point(&digits, 1));
        s.push('e');
        if exponent < 0 {
            s.push_str(&with_sign(&(-(exponent as i64)).to_string(), opts));
        } else {
            s.push_str(&exponent.to_string());
        }
        s
    } else {
        positional(&digits, exponent)
    };
    if x < 0.0 { with_sign(&body, opts) } else { body }
}

/// `digits` written out with the decimal point implied by `exponent`.
fn positional(digits: &str, exponent: i32) -> String {
    if exponent < 0 {
        let zeros = (-exponent - 1) as usize;
        return trim_fraction(&format!("0.{}{}", "0".repeat(zeros), digits));
    }
    let int_len = exponent as usize + 1;
    if int_len >= digits.len() {
        // Rounding put the last significant digit left of the point; the
        // padding zeros carry magnitude, so there is nothing to trim.
        return format!("{}{}", digits, "0".repeat(int_len - digits.len()));
    }
    trim_fraction(&place_point(digits, int_len))
}

/// Insert a decimal point after `int_len` digits.
fn place_point(digits: &str, int_len: usize) -> String {
    format!("{}.{}", &digits[..int_len], &digits[int_len..])
}

/// Drop trailing fraction zeros, then a bare trailing point.
fn trim_fraction(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn with_sign(body: &str, opts: &FmtOpts) -> String {
    let mut s = String::with_capacity(body.len() + opts.neg.len_utf8());
    s.push(opts.neg);
    s.push_str(body);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::Buf;
    use rstest::rstest;

    fn j(a: &Array) -> String {
        format_array(a, &FmtOpts::J)
    }

    fn apl(a: &Array) -> String {
        format_array(a, &FmtOpts::APL)
    }

    fn fj(x: f64) -> String {
        format_f64(x, &FmtOpts::J)
    }

    // Atoms.

    #[rstest]
    #[case(0, "0")]
    #[case(7, "7")]
    #[case(-3, "_3")]
    #[case(-1234, "_1234")]
    #[case(i64::MIN, "_9223372036854775808")]
    fn integers_j(#[case] v: i64, #[case] want: &str) {
        assert_eq!(format_i64(v, &FmtOpts::J), want);
    }

    #[rstest]
    #[case(-3, "¯3")]
    #[case(3, "3")]
    fn integers_apl(#[case] v: i64, #[case] want: &str) {
        assert_eq!(format_i64(v, &FmtOpts::APL), want);
    }

    #[rstest]
    #[case(0.0, "0")]
    #[case(0.5, "0.5")]
    #[case(2.0, "2")]
    #[case(-2.0, "_2")]
    #[case(1.0 / 3.0, "0.333333")]
    #[case(-1.0 / 3.0, "_0.333333")]
    #[case(2.0 / 3.0, "0.666667")]
    #[case(1.25, "1.25")]
    #[case(100.0, "100")]
    #[case(1e-5, "0.00001")]
    #[case(0.000012345678, "0.0000123457")]
    #[case(1e11, "100000000000")]
    #[case(123456789.0, "123457000")]
    fn floats_positional(#[case] x: f64, #[case] want: &str) {
        assert_eq!(fj(x), want);
    }

    #[rstest]
    #[case(1e-7, "1e_7")]
    #[case(-1e-7, "_1e_7")]
    #[case(1.5e13, "1.5e13")]
    #[case(1e12, "1e12")]
    #[case(-2.5e20, "_2.5e20")]
    #[case(1.234567e-9, "1.23457e_9")]
    fn floats_exponent(#[case] x: f64, #[case] want: &str) {
        assert_eq!(fj(x), want);
    }

    #[test]
    fn floats_apl_signs() {
        assert_eq!(format_f64(-0.5, &FmtOpts::APL), "¯0.5");
        assert_eq!(format_f64(1e-7, &FmtOpts::APL), "1e¯7");
        assert_eq!(format_f64(-1e-7, &FmtOpts::APL), "¯1e¯7");
    }

    #[test]
    fn negative_zero_prints_unsigned() {
        assert_eq!(fj(-0.0), "0");
    }

    #[test]
    fn nan_and_infinities() {
        assert_eq!(fj(f64::NAN), "_.");
        assert_eq!(fj(f64::INFINITY), "_");
        assert_eq!(fj(f64::NEG_INFINITY), "__");
        assert_eq!(format_f64(f64::NAN, &FmtOpts::APL), "¯.");
        assert_eq!(format_f64(f64::INFINITY, &FmtOpts::APL), "∞");
        assert_eq!(format_f64(f64::NEG_INFINITY, &FmtOpts::APL), "¯∞");
    }

    #[test]
    fn scalars() {
        assert_eq!(j(&Array::scalar_i64(-3)), "_3");
        assert_eq!(apl(&Array::scalar_i64(-3)), "¯3");
        assert_eq!(j(&Array::scalar_f64(0.5)), "0.5");
        assert_eq!(j(&Array::scalar_bool(true)), "1");
        assert_eq!(j(&Array::scalar_bool(false)), "0");
        assert_eq!(j(&Array::new(vec![], Data::Char(vec!['q'].into()))), "q");
    }

    // Vectors.

    #[test]
    fn integer_vector() {
        let a = Array::from_i64(vec![1, -22, 333]);
        assert_eq!(j(&a), "1 _22 333");
        assert_eq!(apl(&a), "1 ¯22 333");
    }

    #[test]
    fn float_vector_trims_independently() {
        let a = Array::from_f64(vec![0.5, 2.0, 1.0 / 3.0, -1e-7]);
        assert_eq!(j(&a), "0.5 2 0.333333 _1e_7");
    }

    #[test]
    fn bool_vector() {
        let a = Array::new(vec![4], Data::Bool(vec![1, 0, 0, 1].into()));
        assert_eq!(j(&a), "1 0 0 1");
    }

    #[test]
    fn char_vector_is_a_plain_string() {
        let a = Array::from_chars("hello".chars().collect());
        assert_eq!(j(&a), "hello");
    }

    // Matrices.

    #[test]
    fn matrix_columns_align_right() {
        let a = Array::new(vec![2, 3], Data::I64(vec![1, 22, 333, 4444, 5, 66].into()));
        assert_eq!(j(&a), "   1 22 333\n4444  5  66");
    }

    #[test]
    fn matrix_negatives_widen_their_column() {
        let a = Array::new(vec![2, 2], Data::I64(vec![-1, 10, 100, -2].into()));
        assert_eq!(j(&a), " _1 10\n100 _2");
        // `¯` is one column wide even though it is two bytes.
        assert_eq!(apl(&a), " ¯1 10\n100 ¯2");
    }

    #[test]
    fn matrix_of_floats() {
        let a = Array::new(vec![2, 2], Data::F64(vec![0.5, 2.0, -1.0 / 3.0, 10.0].into()));
        assert_eq!(j(&a), "      0.5  2\n_0.333333 10");
    }

    #[test]
    fn matrix_of_bools() {
        let a = Array::new(vec![2, 3], Data::Bool(vec![1, 0, 1, 0, 1, 0].into()));
        assert_eq!(j(&a), "1 0 1\n0 1 0");
    }

    #[test]
    fn single_column_matrix() {
        let a = Array::new(vec![3, 1], Data::I64(vec![1, -20, 300].into()));
        assert_eq!(j(&a), "  1\n_20\n300");
    }

    // Higher rank.

    #[test]
    fn rank_3_separates_planes_with_one_blank_line() {
        let a = Array::new(vec![2, 2, 2], Data::I64(vec![1, 2, 3, 4, 5, 6, 7, 8].into()));
        assert_eq!(j(&a), "1 2\n3 4\n\n5 6\n7 8");
    }

    #[test]
    fn rank_3_column_widths_are_global() {
        let a = Array::new(vec![2, 1, 2], Data::I64(vec![1, 2, 300, 4].into()));
        assert_eq!(j(&a), "  1 2\n\n300 4");
    }

    #[test]
    fn rank_4_separates_groups_with_two_blank_lines() {
        let a = Array::new(vec![2, 2, 1, 2], Data::I64(vec![1, 2, 3, 4, 5, 6, 7, 8].into()));
        assert_eq!(j(&a), "1 2\n\n3 4\n\n\n5 6\n\n7 8");
    }

    #[test]
    fn rank_5_gap_grows_with_the_axis() {
        let a = Array::new(vec![2, 1, 1, 1, 1], Data::I64(vec![1, 2].into()));
        // The step is along axis -5: three blank lines.
        assert_eq!(j(&a), "1\n\n\n\n2");
    }

    #[rstest]
    // Frame [2], rank 3: every step is along axis -3.
    #[case(&[2], 1, 1)]
    // Frame [2, 3], rank 4: within a group one blank, across groups two.
    #[case(&[2, 3], 1, 1)]
    #[case(&[2, 3], 2, 1)]
    #[case(&[2, 3], 3, 2)]
    #[case(&[2, 3], 4, 1)]
    fn plane_gaps(#[case] frame: &[usize], #[case] p: usize, #[case] want: usize) {
        assert_eq!(plane_gap(frame, p), want);
    }

    // Characters at rank 2 and above.

    #[test]
    fn char_matrix_is_lines() {
        let a = Array::new(vec![2, 3], Data::Char("abcdef".chars().collect()));
        assert_eq!(j(&a), "abc\ndef");
    }

    #[test]
    fn char_matrix_keeps_spaces_unpadded() {
        let a = Array::new(vec![2, 3], Data::Char("a  bcd".chars().collect()));
        assert_eq!(j(&a), "a  \nbcd");
    }

    #[test]
    fn char_rank_3_separates_planes() {
        let a = Array::new(vec![2, 2, 2], Data::Char("abcdefgh".chars().collect()));
        assert_eq!(j(&a), "ab\ncd\n\nef\ngh");
    }

    // Boxes.

    fn boxed(shape: &[usize], items: Vec<Array>) -> Array {
        Array::new(shape.to_vec(), Data::Box(items.into()))
    }

    #[test]
    fn a_box_is_drawn_as_a_fenced_cell() {
        let a = boxed(&[], vec![Array::from_i64(vec![1, 2])]);
        assert_eq!(j(&a), "+---+\n|1 2|\n+---+");
        // APL spaces the contents instead of fencing them.
        assert_eq!(apl(&a), " 1 2 ");
    }

    #[test]
    fn a_boxed_vector_is_a_row_of_cells() {
        let a = boxed(
            &[3],
            vec![
                Array::scalar_i64(1),
                Array::from_i64(vec![2, 3]),
                Array::from_chars("abc".chars().collect()),
            ],
        );
        assert_eq!(j(&a), "+-+---+---+\n|1|2 3|abc|\n+-+---+---+");
        // A non-scalar item (the unenclosed vector `2 3`) widens the gap
        // beside it by one column; the character vector costs nothing
        // extra, since a run of characters already reads as text.
        assert_eq!(apl(&a), " 1  2 3  abc ");
    }

    // The nested display rule (GNU-taught, corpus/apl/nested_display.txt
    // carries the oracle-checked cases): a run of adjacent characters is
    // text with no separator; elsewhere the gap widens by the more
    // complex neighbour's own shape, and the outer margin is set by how
    // many boxes wrap the first and the last item.

    #[test]
    fn a_char_run_merges_through_a_box() {
        // `'a',⊂'b'` — a boxed character beside a plain one is still text,
        // whatever `⊂` it is wrapped in.
        let ch = |c: char| Array::new(vec![], Data::Char(vec![c].into()));
        let a = boxed(&[2], vec![ch('a'), boxed(&[], vec![ch('b')])]);
        assert_eq!(apl(&a), "ab");
    }

    #[test]
    fn a_nonscalar_item_widens_its_own_gap() {
        // `1,⊂1 2` — a boxed vector costs one extra column beside a scalar.
        let a = boxed(&[2], vec![Array::scalar_i64(1), Array::from_i64(vec![1, 2])]);
        assert_eq!(apl(&a), " 1  1 2 ");
    }

    #[test]
    fn a_boxed_matrix_costs_two_columns() {
        // `1,⊂2 2⍴1 2 3 4` — rank widens the gap further than a vector does.
        let a = boxed(
            &[2],
            vec![Array::scalar_i64(1), Array::new(vec![2, 2], Data::I64(vec![1, 2, 3, 4].into()))],
        );
        assert_eq!(apl(&a), " 1   1 2 \n     3 4 ");
    }

    #[test]
    fn a_character_vector_costs_one_column_less_than_its_rank() {
        // `1,⊂'abc'` — text needs no extra column even though it is rank 1.
        let a = boxed(&[2], vec![Array::scalar_i64(1), Array::from_chars("abc".chars().collect())]);
        assert_eq!(apl(&a), " 1 abc ");
    }

    #[test]
    fn the_outer_margin_follows_the_edge_items_own_box_depth() {
        // `⊂⊂1 2,1` — a doubly-enclosed first item widens the lead margin.
        let inner = boxed(&[], vec![Array::from_i64(vec![1, 2])]);
        let a = boxed(&[2], vec![boxed(&[], vec![inner]), Array::scalar_i64(1)]);
        assert_eq!(apl(&a), "  1 2  1 ");
    }

    #[test]
    fn a_tall_cell_pads_the_others_below_it() {
        let a = boxed(
            &[2],
            vec![
                Array::scalar_i64(1),
                Array::new(vec![2, 2], Data::I64(vec![1, 2, 3, 4].into())),
            ],
        );
        assert_eq!(j(&a), "+-+---+\n|1|1 2|\n| |3 4|\n+-+---+");
    }

    #[test]
    fn a_nested_box_draws_inside_its_cell() {
        let inner = boxed(&[], vec![Array::scalar_i64(5)]);
        assert_eq!(j(&boxed(&[], vec![inner])), "+---+\n|+-+|\n||5||\n|+-+|\n+---+");
    }

    #[test]
    fn a_box_matrix_fences_every_row() {
        let a = boxed(&[2, 2], (1..=4).map(Array::scalar_i64).collect());
        assert_eq!(j(&a), "+-+-+\n|1|2|\n+-+-+\n|3|4|\n+-+-+");
        // Every element is a simple scalar, which APL reads as a mixed
        // SIMPLE array: it draws like a plain one.
        assert_eq!(apl(&a), "1 2\n3 4");
    }

    #[test]
    fn a_boxed_empty_is_a_cell_of_width_zero() {
        let a = boxed(&[], vec![Array::empty(DType::I64)]);
        assert_eq!(j(&a), "++\n||\n++");
        // A boxed array with an empty axis shows nothing at all.
        assert_eq!(j(&Array::new(vec![0], Data::Box(Buf::new()))), "");
    }

    // Empties.

    #[rstest]
    #[case(DType::Bool)]
    #[case(DType::I64)]
    #[case(DType::F64)]
    #[case(DType::Char)]
    fn empty_vectors_print_nothing(#[case] dtype: DType) {
        assert_eq!(j(&Array::empty(dtype)), "");
    }

    #[rstest]
    #[case(&[0, 3])]
    #[case(&[3, 0])]
    #[case(&[2, 0, 4])]
    fn any_empty_axis_prints_nothing(#[case] shape: &[usize]) {
        let a = Array::new(shape.to_vec(), Data::I64(vec![].into()));
        assert_eq!(j(&a), "");
    }

    #[test]
    fn no_trailing_newline_or_spaces() {
        let a = Array::new(vec![2, 2, 2], Data::I64(vec![1, 22, 3, 4, 5, 6, 7, 8].into()));
        let s = j(&a);
        assert!(!s.ends_with('\n'));
        for line in s.lines() {
            assert_eq!(line.trim_end(), line, "line has trailing space: {line:?}");
        }
    }
}
