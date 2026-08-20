//! Human-readable array formatting, J session style: numeric columns
//! aligned, higher-rank arrays printed as planes separated by blank lines.

use crate::array::{Array, Data};
use crate::dtype::DType;

/// Display conventions that differ between languages.
#[derive(Clone, Copy, Debug)]
pub struct FmtOpts {
    /// Negative-number prefix: `_` for J, `¯` for APL.
    pub neg: char,
}

impl FmtOpts {
    pub const J: FmtOpts = FmtOpts { neg: '_' };
    pub const APL: FmtOpts = FmtOpts { neg: '¯' };
}

/// Significant digits kept when displaying a float.
const SIG_DIGITS: usize = 6;

/// Format an array for display. No trailing newline.
pub fn format_array(a: &Array, opts: &FmtOpts) -> String {
    // An array with an empty axis has nothing to show.
    if a.shape.contains(&0) {
        return String::new();
    }
    let rank = a.rank();
    let texts: Vec<String> = (0..a.count()).map(|i| format_atom(&a.data, i, opts)).collect();
    // Characters are laid out as text lines; everything else as columns.
    let text_layout = a.dtype() == DType::Char;
    match rank {
        0 => texts.into_iter().next().unwrap_or_default(),
        1 if text_layout => texts.concat(),
        1 => texts.join(" "),
        _ => {
            let ncols = a.shape[rank - 1];
            let nrows = a.shape[rank - 2];
            // Column widths span every plane, so planes stay aligned with
            // each other and not just internally.
            let widths = if text_layout { vec![0; ncols] } else { column_widths(&texts, ncols) };
            let frame = &a.shape[..rank - 2];
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
                    push_row(&mut out, &texts[start..start + ncols], &widths, text_layout);
                }
            }
            out
        }
    }
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
fn column_widths(texts: &[String], ncols: usize) -> Vec<usize> {
    let mut widths = vec![0usize; ncols];
    for (i, t) in texts.iter().enumerate() {
        let j = i % ncols;
        widths[j] = widths[j].max(width(t));
    }
    widths
}

fn push_row(out: &mut String, row: &[String], widths: &[usize], text_layout: bool) {
    for (j, cell) in row.iter().enumerate() {
        if text_layout {
            out.push_str(cell);
            continue;
        }
        if j > 0 {
            out.push(' ');
        }
        for _ in 0..widths[j].saturating_sub(width(cell)) {
            out.push(' ');
        }
        out.push_str(cell);
    }
}

/// Display width in characters; the APL minus sign is multi-byte.
fn width(s: &str) -> usize {
    s.chars().count()
}

fn format_atom(data: &Data, i: usize, opts: &FmtOpts) -> String {
    match data {
        Data::Bool(v) => (if v[i] != 0 { "1" } else { "0" }).to_string(),
        Data::I64(v) => format_i64(v[i], opts),
        Data::F64(v) => format_f64(v[i], opts),
        Data::Char(v) => v[i].to_string(),
    }
}

fn format_i64(v: i64, opts: &FmtOpts) -> String {
    let s = v.to_string();
    match s.strip_prefix('-') {
        Some(digits) => with_sign(digits, opts),
        None => s,
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
        assert_eq!(j(&Array::new(vec![], Data::Char(vec!['q']))), "q");
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
        let a = Array::new(vec![4], Data::Bool(vec![1, 0, 0, 1]));
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
        let a = Array::new(vec![2, 3], Data::I64(vec![1, 22, 333, 4444, 5, 66]));
        assert_eq!(j(&a), "   1 22 333\n4444  5  66");
    }

    #[test]
    fn matrix_negatives_widen_their_column() {
        let a = Array::new(vec![2, 2], Data::I64(vec![-1, 10, 100, -2]));
        assert_eq!(j(&a), " _1 10\n100 _2");
        // `¯` is one column wide even though it is two bytes.
        assert_eq!(apl(&a), " ¯1 10\n100 ¯2");
    }

    #[test]
    fn matrix_of_floats() {
        let a = Array::new(vec![2, 2], Data::F64(vec![0.5, 2.0, -1.0 / 3.0, 10.0]));
        assert_eq!(j(&a), "      0.5  2\n_0.333333 10");
    }

    #[test]
    fn matrix_of_bools() {
        let a = Array::new(vec![2, 3], Data::Bool(vec![1, 0, 1, 0, 1, 0]));
        assert_eq!(j(&a), "1 0 1\n0 1 0");
    }

    #[test]
    fn single_column_matrix() {
        let a = Array::new(vec![3, 1], Data::I64(vec![1, -20, 300]));
        assert_eq!(j(&a), "  1\n_20\n300");
    }

    // Higher rank.

    #[test]
    fn rank_3_separates_planes_with_one_blank_line() {
        let a = Array::new(vec![2, 2, 2], Data::I64(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(j(&a), "1 2\n3 4\n\n5 6\n7 8");
    }

    #[test]
    fn rank_3_column_widths_are_global() {
        let a = Array::new(vec![2, 1, 2], Data::I64(vec![1, 2, 300, 4]));
        assert_eq!(j(&a), "  1 2\n\n300 4");
    }

    #[test]
    fn rank_4_separates_groups_with_two_blank_lines() {
        let a = Array::new(vec![2, 2, 1, 2], Data::I64(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(j(&a), "1 2\n\n3 4\n\n\n5 6\n\n7 8");
    }

    #[test]
    fn rank_5_gap_grows_with_the_axis() {
        let a = Array::new(vec![2, 1, 1, 1, 1], Data::I64(vec![1, 2]));
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
        let a = Array::new(shape.to_vec(), Data::I64(vec![]));
        assert_eq!(j(&a), "");
    }

    #[test]
    fn no_trailing_newline_or_spaces() {
        let a = Array::new(vec![2, 2, 2], Data::I64(vec![1, 22, 3, 4, 5, 6, 7, 8]));
        let s = j(&a);
        assert!(!s.ends_with('\n'));
        for line in s.lines() {
            assert_eq!(line.trim_end(), line, "line has trailing space: {line:?}");
        }
    }
}
