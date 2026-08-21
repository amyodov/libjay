//! Column-major arguments answer exactly what row-major ones answer.
//!
//! An array carries the layout of its buffer, and a table imported from
//! Arrow — or anything transposed with `|:` — arrives column-major. A few
//! verbs read that layout where it lies; every other verb is handed the
//! rows, materialised once. Either way the value is the same value, and
//! this file is what holds the runtime to that: the whole primitive table
//! over the same data in both layouts, compared element for element.
//!
//! The battery also runs in debug, where the structural accessors assert
//! that nothing reads a column-major buffer as if it were rows: a verb that
//! quietly assumed the layout fails here rather than answering nonsense.

use jay::{compile, Array, Data, Dialect, Lang, Layout};

/// A deterministic value stream: splitmix64, so every run sees the same data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn f64s(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| (self.next() >> 11) as f64 / (1u64 << 53) as f64 - 0.5).collect()
    }

    fn i64s(&mut self, n: usize, span: i64) -> Vec<i64> {
        (0..n).map(|_| (self.next() % (2 * span as u64 + 1)) as i64 - span).collect()
    }
}

fn run(lang: Lang, src: &str, args: &[Array]) -> Result<Option<Array>, String> {
    let program = compile(lang, src, &Dialect::default()).map_err(|e| e.to_string())?;
    let mut sink = |_: &str| {};
    program.run(args, &mut sink).map_err(|e| e.to_string())
}

/// The same elements as `a`, in a buffer that holds its first axis fastest.
fn as_columns(a: &Array) -> Array {
    let (rows, cols) = (a.shape[0], a.shape[1]);
    let mut out = Data::empty(a.dtype());
    for j in 0..cols {
        for i in 0..rows {
            out.push_from(&a.data, i * cols + j);
        }
    }
    let laid = Array::col_major(a.shape.clone(), out);
    assert_eq!(laid.layout(), Layout::ColMajor);
    laid
}

/// Two results agree: exactly for the discrete types, to 1e-12 relative for
/// the float and complex ones, which a regrouped fold is allowed to differ
/// in (§5.9).
fn agree(what: &str, a: &Array, b: &Array) {
    assert_eq!(a.shape, b.shape, "{what}: shapes differ");
    assert_eq!(a.dtype(), b.dtype(), "{what}: types differ");
    let (a, b) = (a.to_row_major(), b.to_row_major());
    match (&a.data, &b.data) {
        (Data::F64(x), Data::F64(y)) => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                // Relative to the values, with an absolute floor: a sum
                // of values around 1 that cancels to nearly nothing is
                // exact to the last bits of the terms, not of the answer.
                let scale = p.abs().max(q.abs()).max(1.0);
                assert!((p - q).abs() <= 1e-12 * scale, "{what}: element {i}: {p} vs {q}");
            }
        }
        (x, y) => assert_eq!(x, y, "{what}: values differ"),
    }
}

/// Every program below is applied to one matrix. They are the reductions,
/// the scans and windows, the structural verbs, the elementwise ones, the
/// chains a kernel fuses, the boxing verbs and the ones that only read a
/// shape — that is, every shape of consumer the layout has to survive.
const PROGRAMS: &[&str] = &[
    // reductions along the leading axis, and over the rows
    "+/ {m}",
    "*/ {m}",
    "-/ {m}",
    ">./ {m}",
    "<./ {m}",
    "+/\"1 {m}",
    "*/\"1 {m}",
    "-/\"1 {m}",
    ">./\"1 {m}",
    "+/ +/ {m}",
    "+/ , {m}",
    // shape and count
    "$ {m}",
    "# {m}",
    "# , {m}",
    "$ |: {m}",
    // structural
    "|: {m}",
    "|: |: {m}",
    ", {m}",
    ",. {m}",
    "|. {m}",
    "|.\"1 {m}",
    "{. {m}",
    "{: {m}",
    "}. {m}",
    "}: {m}",
    "1 |. {m}",
    "1 {. {m}",
    "_1 }. {m}",
    "0 { {m}",
    "0 {\"1 {m}",
    "1 |.\"1 {m}",
    "2 2 $ {m}",
    "/: +/\"1 {m}",
    "~. , {m}",
    "{m} , {m}",
    "{m} ,. {m}",
    "{m} , |: {m}",
    // elementwise, one argument and two
    "- {m}",
    "*: {m}",
    "| {m}",
    "<. {m}",
    "^ {m}",
    "{m} + {m}",
    "{m} * {m}",
    "{m} - |: |: {m}",
    "2 * {m}",
    "{m} > 0",
    "{m} + 1 2 3 4",
    // chains a kernel fuses, mapping and reducing
    "({m} * {m}) + 1",
    "1 + 2 * {m} + {m}",
    "+/ , {m} * {m}",
    "+/\"1 ({m} * 2) + 1",
    // scans and windows along the leading axis
    "+/\\ {m}",
    "2 +/\\ {m}",
    ">./\\ {m}",
    "+/\\. {m}",
    // boxes
    "< {m}",
    "> < {m}",
    "{m} ; {m}",
    "< |: {m}",
    "#: 0 { , {m}",
    // formatting reads every element in order
    "\": {m}",
    // sorting and searching
    "/: {m}",
    "\\: {m}",
    "(0 { {m}) i.\"1 {m}",
];

const APL_PROGRAMS: &[&str] = &[
    "+⌿{m}",
    "⌈⌿{m}",
    "+/{m}",
    "⍉{m}",
    "⍴{m}",
    "≢{m}",
    ",{m}",
    "⌽{m}",
    "⊖{m}",
    "{m}+{m}",
    "2×{m}",
    "+⌿{m}×{m}",
    "⊂{m}",
    "↑{m}",
    "1↓{m}",
    "+\\{m}",
];

fn battery(lang: Lang, programs: &[&str], rows: usize, cols: usize, values: Data) {
    let m = Array::new(vec![rows, cols], values);
    let columns = as_columns(&m);
    for src in programs {
        let what = format!("{src} over {rows}x{cols} {}", m.dtype().name());
        let by_rows = run(lang, src, std::slice::from_ref(&m));
        let by_columns = run(lang, src, std::slice::from_ref(&columns));
        match (by_rows, by_columns) {
            (Ok(Some(a)), Ok(Some(b))) => agree(&what, &a, &b),
            (Ok(None), Ok(None)) => {}
            (Err(a), Err(b)) => assert_eq!(a, b, "{what}: different errors"),
            (a, b) => panic!("{what}: one layout answered and the other did not: {a:?} / {b:?}"),
        }
    }
}

/// The subset worth running at a size that reaches the parallel split, the
/// lane thresholds and the per-column fold: the folds, the passes and the
/// kernels. The rest of the table is quadratic in places (a nub, a grade, a
/// search) and says what it has to say at four by five.
const LARGE_PROGRAMS: &[&str] = &[
    "+/ {m}",
    "*/ {m}",
    "-/ {m}",
    ">./ {m}",
    "+/\"1 {m}",
    "-/\"1 {m}",
    "+/ , {m}",
    "$ {m}",
    "|: {m}",
    ", {m}",
    "|. {m}",
    "1 |. {m}",
    "0 { {m}",
    "*: {m}",
    "{m} + {m}",
    "{m} > 0",
    "({m} * {m}) + 1",
    "+/ , {m} * {m}",
    "+/\\ {m}",
    "2 +/\\ {m}",
];

#[test]
fn the_folds_and_the_kernels_agree_at_a_size_that_splits() {
    let mut rng = Rng(29);
    // Tall enough that a column of its own reaches the parallel fold, and
    // wide-and-short so that the other split is taken too.
    for &(rows, cols) in &[(70_000usize, 3usize), (3, 70_000), (300, 200)] {
        let n = rows * cols;
        battery(Lang::J, LARGE_PROGRAMS, rows, cols, Data::F64(rng.f64s(n).into()));
        battery(Lang::J, LARGE_PROGRAMS, rows, cols, Data::I64(rng.i64s(n, 9).into()));
    }
    battery(Lang::Apl, APL_PROGRAMS, 300, 200, Data::F64(rng.f64s(60_000).into()));
}

#[test]
fn every_primitive_answers_the_same_over_columns_as_over_rows() {
    let mut rng = Rng(7);
    for &(rows, cols) in &[(1usize, 1usize), (2, 3), (5, 4), (4, 5)] {
        let n = rows * cols;
        battery(Lang::J, PROGRAMS, rows, cols, Data::F64(rng.f64s(n).into()));
        battery(Lang::J, PROGRAMS, rows, cols, Data::I64(rng.i64s(n, 9).into()));
        battery(Lang::Apl, APL_PROGRAMS, rows, cols, Data::F64(rng.f64s(n).into()));
        battery(Lang::Apl, APL_PROGRAMS, rows, cols, Data::I64(rng.i64s(n, 9).into()));
    }
}


#[test]
fn a_boolean_table_folds_the_same_either_way() {
    let mut rng = Rng(11);
    let n = 3 * 7;
    let bits: Vec<u8> = rng.i64s(n, 1).iter().map(|&x| (x > 0) as u8).collect();
    battery(Lang::J, PROGRAMS, 3, 7, Data::Bool(bits.into()));
}

#[test]
fn a_higher_rank_transpose_is_still_the_same_value() {
    let mut rng = Rng(13);
    let m = Array::new(vec![2, 3, 4], Data::F64(rng.f64s(24).into()));
    for src in ["|: {m}", "+/ |: {m}", "$ |: {m}", ", |: {m}", "|: |: {m}", "+/\"1 |: {m}"] {
        let a = run(Lang::J, src, std::slice::from_ref(&m)).expect("run").expect("a value");
        // The same program over an argument the runtime cannot keep a flag
        // for: the elements, laid out, through a ravel and a reshape.
        let laid = Array::new(m.shape.clone(), m.to_row_major().data.clone());
        let b = run(Lang::J, src, &[laid]).expect("run").expect("a value");
        agree(src, &a, &b);
    }
}

#[test]
fn transposing_a_matrix_moves_no_elements() {
    let mut rng = Rng(17);
    let m = Array::new(vec![1000, 8], Data::F64(rng.f64s(8000).into()));
    let before = m.as_f64_slice().expect("floats").as_ptr();
    let r = run(Lang::J, "|: {m}", std::slice::from_ref(&m)).expect("run").expect("a value");
    assert_eq!(r.shape, vec![8, 1000]);
    assert_eq!(r.layout(), Layout::ColMajor);
    assert_eq!(r.as_f64_slice().expect("floats").as_ptr(), before, "the transpose copied");
    // And back again: two transposes are the original buffer, row-major.
    let r = run(Lang::J, "|: |: {m}", std::slice::from_ref(&m)).expect("run").expect("a value");
    assert_eq!(r.layout(), Layout::RowMajor);
    assert_eq!(r.as_f64_slice().expect("floats").as_ptr(), before);
}

#[test]
fn a_column_major_argument_folds_its_columns_where_they_lie() {
    let mut rng = Rng(19);
    let (rows, cols) = (5000, 6);
    let m = Array::new(vec![rows, cols], Data::F64(rng.f64s(rows * cols).into()));
    let columns = as_columns(&m);
    let before = columns.as_f64_slice().expect("floats").as_ptr();
    // The leading-axis fold and the row fold both read the columns as they
    // lie, so neither of them makes a copy of the argument to work from.
    for src in ["+/ {m}", "+/\"1 {m}", "$ {m}", "# {m}", "|: {m}", "- {m}", "{m} + {m}"] {
        let r = run(Lang::J, src, std::slice::from_ref(&columns)).expect("run").expect("a value");
        agree(src, &r, &run(Lang::J, src, std::slice::from_ref(&m)).expect("run").expect("a value"));
        assert_eq!(columns.as_f64_slice().expect("floats").as_ptr(), before, "{src}: argument moved");
    }
}

#[test]
fn an_explanation_says_when_a_value_is_column_major() {
    let mut rng = Rng(23);
    let m = Array::new(vec![4, 3], Data::F64(rng.f64s(12).into()));
    let program = compile(Lang::J, "+/ |: {m}", &Dialect::default()).expect("compile");
    let text = program.explain(Some(&[m]));
    assert!(text.contains("column-major"), "the explanation hid the layout:\n{text}");
}

/// The whole point of the layout, at the level the boundary works at: a
/// table whose columns are borrowed from someone else's memory is folded
/// where it lies, and the columns are never joined into one block.
#[test]
fn a_table_of_borrowed_columns_is_folded_without_a_copy() {
    use std::sync::Arc;

    let columns: Arc<Vec<Vec<f64>>> =
        Arc::new((0..4).map(|j| (0..1000).map(|i| (i * 4 + j) as f64).collect()).collect());
    let parts: Vec<jay::Buf<f64>> = (0..4)
        .map(|j| {
            let owner: Arc<dyn std::any::Any + Send + Sync> = columns.clone();
            // SAFETY: the vectors live inside the `Arc` each buffer holds a
            // clone of, and nothing mutates them for as long as it lives.
            unsafe { jay::Buf::foreign(columns[j].as_ptr(), columns[j].len(), owner) }
        })
        .collect();
    let table = Array::col_major(vec![1000, 4], Data::F64(jay::Buf::join(parts)));
    assert!(table.data.is_foreign(), "the columns were copied on the way in");

    let sums = run(Lang::J, "+/ {m}", std::slice::from_ref(&table)).expect("run").expect("a value");
    let rows = run(Lang::J, "+/\"1 {m}", std::slice::from_ref(&table)).expect("run").expect("a value");
    let shape = run(Lang::J, "$ {m}", std::slice::from_ref(&table)).expect("run").expect("a value");
    // The join is shared by every clone of the buffer, so this says that
    // none of the three runs made one.
    assert!(table.data.is_foreign(), "the columns were joined into one block");

    // And the values are the ones the same table gives as rows.
    let by_rows = table.to_row_major();
    agree("+/", &sums, &run(Lang::J, "+/ {m}", std::slice::from_ref(&by_rows)).unwrap().unwrap());
    agree("+/\"1", &rows, &run(Lang::J, "+/\"1 {m}", &[by_rows]).unwrap().unwrap());
    assert_eq!(shape.to_row_major().data, Data::I64(vec![1000, 4].into()));
}
