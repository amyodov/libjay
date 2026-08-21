"""The data boundary: Arrow and numpy in, Arrow out, and the refusals."""

import datetime
import gc

import pytest

import jay
from jay import JayError, j


class TestNumpy:
    @pytest.fixture(autouse=True)
    def _np(self):
        self.np = pytest.importorskip("numpy")

    def test_i64_vector(self):
        a = self.np.arange(100_000, dtype=self.np.int64)
        assert j("+/ {x}", {"x": a}) == 4999950000

    def test_f64_vector(self):
        a = self.np.arange(100_000, dtype=self.np.float64)
        assert j("+/ {x}", {"x": a}) == pytest.approx(4999950000.0)

    def test_shape_survives(self):
        a = self.np.arange(6, dtype=self.np.int64).reshape(2, 3)
        v = j("] {x}", {"x": a})
        assert v.shape == (2, 3)
        assert v.tolist() == [[0, 1, 2], [3, 4, 5]]

    def test_matrix_reduces_along_the_leading_axis(self):
        a = self.np.arange(6, dtype=self.np.int64).reshape(2, 3)
        assert j("+/ {x}", {"x": a}).tolist() == [3, 5, 7]
        assert j('+/"1 {x}', {"x": a}).tolist() == [3, 12]

    def test_matrix_product_over_borrowed_blocks(self):
        # `+/ . *` (APL `+.×`) reads both numpy blocks where they lie and
        # answers what the reference BLAS would.
        a = self.np.arange(12, dtype=self.np.float64).reshape(3, 4)
        b = self.np.arange(20, dtype=self.np.float64).reshape(4, 5)
        want = (a @ b).ravel().tolist()
        got = j("{x} +/ . * {y}", {"x": a, "y": b})
        assert got.shape == (3, 5)
        flat = [v for row in got.tolist() for v in row]
        assert flat == pytest.approx(want)
        apl_out = jay.apl("{x}+.×{y}", {"x": a, "y": b}).tolist()
        assert [v for row in apl_out for v in row] == pytest.approx(want)

    def test_whole_matrix_product_stays_whole(self):
        a = self.np.arange(6, dtype=self.np.int64).reshape(2, 3)
        b = self.np.arange(6, dtype=self.np.int64).reshape(3, 2)
        assert j("{x} +/ . * {y}", {"x": a, "y": b}).tolist() == [[10, 13], [28, 40]]

    def test_borrowed_memory_is_not_copied(self):
        # The result of `]` is the input array; writing through numpy is
        # visible to it, which is what zero-copy means.
        a = self.np.arange(4, dtype=self.np.int64)
        v = j("] {x}", {"x": a})
        a[0] = 99
        assert v.tolist()[0] == 99

    def test_transposed_view_is_read_where_it_lies(self):
        # A transposed view is contiguous in the other order, which libjay
        # carries rather than refusing: the answer is the same and no copy
        # is made of the block.
        a = self.np.arange(6, dtype=self.np.int64).reshape(2, 3)
        assert j("$ {x}", {"x": a.T}).tolist() == [3, 2]
        assert j("+/ {x}", {"x": a.T}).tolist() == [3, 12]
        assert j("] {x}", {"x": a.T}).tolist() == [[0, 3], [1, 4], [2, 5]]

    def test_a_view_that_is_neither_order_is_refused(self):
        a = self.np.arange(24, dtype=self.np.int64).reshape(2, 3, 4)
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a.transpose(1, 0, 2)})
        assert "not contiguous" in str(e.value)
        assert ".copy()" in str(e.value)

    def test_sliced_view_is_refused(self):
        a = self.np.arange(10, dtype=self.np.int64)
        with pytest.raises(JayError, match="not contiguous"):
            j("+/ {x}", {"x": a[::2]})

    def test_a_copy_of_the_view_works(self):
        a = self.np.arange(6, dtype=self.np.int64).reshape(2, 3)
        assert j("+/ {x}", {"x": a.T.copy()}).tolist() == [3, 12]

    @pytest.mark.parametrize(
        "dtype,total", [("int8", 6), ("int16", 6), ("int32", 6), ("uint8", 6), ("uint32", 6)]
    )
    def test_narrow_integers_widen(self, dtype, total):
        a = self.np.array([1, 2, 3], dtype=dtype)
        assert j("+/ {x}", {"x": a}) == total

    def test_float32_widens(self):
        a = self.np.array([1.5, 2.5], dtype=self.np.float32)
        assert j("+/ {x}", {"x": a}) == pytest.approx(4.0)

    def test_bool_arrives_as_boolean(self):
        a = self.np.array([True, False, True])
        assert j("+/ {x}", {"x": a}) == 2

    def test_uint64_that_does_not_fit_is_refused(self):
        a = self.np.array([2**63], dtype=self.np.uint64)
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a})
        assert "9223372036854775808" in str(e.value)

    def test_unsupported_dtype_is_refused(self):
        a = self.np.array([1.0, 2.0], dtype=self.np.float16)
        with pytest.raises(JayError, match="not supported yet"):
            j("+/ {x}", {"x": a})

    def test_unaligned_block_is_refused(self):
        # np.frombuffer over an offset buffer is contiguous but not aligned
        # for its element type, and reading it as a slice would be undefined.
        raw = bytearray(8 * 5 + 1)
        raw[1:] = self.np.arange(5, dtype=self.np.int64).tobytes()
        a = self.np.frombuffer(memoryview(raw)[1:], dtype=self.np.int64)
        assert not a.flags.aligned
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a})
        assert "not aligned" in str(e.value)
        assert ".copy()" in str(e.value)
        assert j("+/ {x}", {"x": a.copy()}) == 10

    def test_keepalive_across_deletion(self):
        a = self.np.arange(1000, dtype=self.np.int64)
        kernel = jay.j.compile("+/ {x}", {"x": a})
        borrowed = j("] {x}", {"x": a})
        del a
        gc.collect()
        assert kernel() == 499500
        assert borrowed.tolist()[-1] == 999


class TestPyArrow:
    @pytest.fixture(autouse=True)
    def _pa(self):
        self.pa = pytest.importorskip("pyarrow")

    def test_int64_array_in(self):
        assert j("+/ {x}", {"x": self.pa.array([1, 2, 3])}) == 6

    def test_float64_array_in(self):
        assert j("+/ {x}", {"x": self.pa.array([1.5, 2.5])}) == pytest.approx(4.0)

    def test_chunked_array_in(self):
        chunked = self.pa.chunked_array([[1, 2], [3, 4]])
        assert j("+/ {x}", {"x": chunked}) == 10

    def test_result_goes_back_out(self):
        out = self.pa.array(j("i. 5"))
        assert out.type == self.pa.int64()
        assert out.to_pylist() == [0, 1, 2, 3, 4]

    def test_float_result_goes_back_out(self):
        out = self.pa.array(j("1 2 3 % 2"))
        assert out.to_pylist() == [0.5, 1.0, 1.5]

    def test_boolean_result_goes_back_out(self):
        out = self.pa.array(j("1 2 3 > 2"))
        assert out.to_pylist() == [False, False, True]

    def test_matrix_result_cannot_be_exported_yet(self):
        with pytest.raises(JayError) as e:
            self.pa.array(j("i. 2 3"))
        assert "tolist()" in str(e.value)

    def test_exported_buffer_outlives_the_value(self):
        # Exporting hands Arrow the elements themselves, so the consumer
        # keeps them alive: the value they came from can go first.
        value = j("i. 1000")
        out = self.pa.array(value)
        del value
        gc.collect()
        assert out.to_pylist()[-1] == 999
        assert self.pa.compute.sum(out).as_py() == 499500

    def test_capsules_in_the_wrong_order_are_refused(self):
        # The capsule's name is the only thing that says what the pointer
        # inside it is; a producer that swaps the pair must be an error and
        # not a write through the wrong type.
        source = self.pa.array([1, 2, 3])

        class Swapped:
            def __arrow_c_array__(self, requested_schema=None):
                schema, array = source.__arrow_c_array__()
                return array, schema

        with pytest.raises(JayError, match="capsule"):
            j("+/ {x}", {"x": Swapped()})
        assert j("+/ {x}", {"x": source}) == 6

    def test_null_is_refused(self):
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": self.pa.array([1, None, 3])})
        assert "null(s)" in str(e.value)

    def test_decimal_is_not_supported_yet(self):
        column = self.pa.array(
            [1, 2], type=self.pa.decimal128(10, 2)
        )
        with pytest.raises(JayError, match="not supported yet"):
            j("+/ {x}", {"x": column})


class TestPolars:
    @pytest.fixture(autouse=True)
    def _pl(self):
        self.pl = pytest.importorskip("polars")

    def test_series_in(self):
        assert j("+/ {x}", {"x": self.pl.Series("close", [1.0, 2.0, 3.5])}) == 6.5

    def test_series_out(self):
        out = self.pl.Series(j("i. 5"))
        assert out.to_list() == [0, 1, 2, 3, 4]

    def test_null_names_the_column(self):
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": self.pl.Series("close", [1.0, None, 3.5])})
        message = str(e.value)
        assert "column 'close'" in message
        assert "1 null(s)" in message
        assert "fill_null" in message

    def test_dataframe_is_a_matrix_with_rows_leading(self):
        df = self.pl.DataFrame({"a": [1, 2, 3], "b": [10, 20, 30]})
        assert j("$ {df}", {"df": df}).tolist() == [3, 2]
        assert j("+/ {df}", {"df": df}).tolist() == [6, 60]
        assert j('+/"1 {df}', {"df": df}).tolist() == [11, 22, 33]

    def test_dataframe_in_one_call(self):
        df = self.pl.DataFrame({"a": [1.0, 2.0], "b": [10.0, 20.0]})
        assert jay.j("+/ {df}", {"df": df}).tolist() == [3.0, 30.0]

    def test_single_column_frame_is_a_vector(self):
        df = self.pl.DataFrame({"a": [1, 2, 3]})
        assert j("$ {df}", {"df": df}).tolist() == [3]

    def test_mixed_dtypes_are_refused_with_a_cast_suggestion(self):
        df = self.pl.DataFrame({"volume": [1, 2], "close": [1.0, 2.0]})
        with pytest.raises(JayError) as e:
            j("+/ {df}", {"df": df})
        message = str(e.value)
        assert "columns disagree" in message
        assert "'volume' is int64" in message
        assert "'close' is float64" in message
        assert "volume.cast(Float64)" in message

    def test_string_column_is_not_supported_yet(self):
        df = self.pl.DataFrame({"sym": ["a", "b"], "n": [1, 2]})
        with pytest.raises(JayError) as e:
            j("+/ {df}", {"df": df})
        message = str(e.value)
        assert "column 'sym'" in message
        assert "yet" in message

    def test_timestamps_are_plain_integers(self):
        day = datetime.datetime(2020, 1, 2) - datetime.datetime(2020, 1, 1)
        column = self.pl.Series(
            "t", [datetime.datetime(2020, 1, 1), datetime.datetime(2020, 1, 2)]
        )
        # Polars stores microseconds; the difference is integer arithmetic.
        assert j("-/ {x}", {"x": column}) == -int(day.total_seconds() * 1_000_000)

    def test_a_dataframe_crosses_without_a_copy(self):
        # The columns are borrowed where Arrow put them and folded there:
        # a column sum, a row sum and the shape all answer without joining
        # the table into one block.
        df = self.pl.DataFrame(
            {"a": [1.0, 2.0, 3.0], "b": [10.0, 20.0, 30.0], "c": [4.0, 5.0, 6.0]}
        )
        before = jay._jay.joins_made()
        assert j("$ {df}", {"df": df}).tolist() == [3, 3]
        assert j("# {df}", {"df": df}) == 3
        assert j("+/ {df}", {"df": df}).tolist() == [6.0, 60.0, 15.0]
        assert j('+/"1 {df}', {"df": df}).tolist() == [15.0, 27.0, 39.0]
        assert j("+/ +/ {df}", {"df": df}) == 81.0
        assert jay._jay.joins_made() == before, "the table was copied"

    def test_a_verb_that_wants_rows_gets_them_once(self):
        # The counterpart: `,` reads the elements in row-major order, so the
        # table is laid out — once, and only for the verbs that need it.
        df = self.pl.DataFrame({"a": [1.0, 2.0], "b": [10.0, 20.0]})
        before = jay._jay.layouts_made()
        assert j("+/ {df}", {"df": df}).tolist() == [3.0, 30.0]
        assert jay._jay.layouts_made() == before, "the fold laid out the rows"
        assert j(", {df}", {"df": df}).tolist() == [1.0, 10.0, 2.0, 20.0]
        assert jay._jay.layouts_made() > before

    def test_column_sums_match_polars(self):
        df = self.pl.DataFrame(
            {"open": [1.0, 2.0, 3.0], "close": [1.5, 2.5, 3.5]}
        )
        expected = [df["open"].sum(), df["close"].sum()]
        assert j("+/ {df}", {"df": df}).tolist() == pytest.approx(expected)


class TestPandas:
    @pytest.fixture(autouse=True)
    def _pd(self):
        self.pd = pytest.importorskip("pandas")
        if not hasattr(self.pd.DataFrame, "__arrow_c_stream__"):
            pytest.skip("pandas without the Arrow stream interface")

    def test_dataframe_in(self):
        df = self.pd.DataFrame({"a": [1, 2, 3], "b": [10, 20, 30]})
        assert j("+/ {df}", {"df": df}).tolist() == [6, 60]
        assert j('+/"1 {df}', {"df": df}).tolist() == [11, 22, 33]

    def test_mixed_dtypes_are_refused(self):
        df = self.pd.DataFrame({"volume": [1, 2], "close": [1.0, 2.0]})
        with pytest.raises(JayError, match="columns disagree"):
            j("+/ {df}", {"df": df})

    def test_numeric_dataframe_column_sums(self):
        df = self.pd.DataFrame({"a": [1.0, 2.0], "b": [3.0, 4.0]})
        assert j("+/ {df}", {"df": df}).tolist() == pytest.approx([3.0, 7.0])


class TestComplex:
    """Complex values at the boundary: numpy's complex128 in, Python's
    ``complex`` out, and Arrow's ``struct<re, im>`` both ways — which is the
    single-array form of the paired-column convention, since Arrow has no
    complex type of its own."""

    @pytest.fixture(autouse=True)
    def _np(self):
        self.np = pytest.importorskip("numpy")

    def test_complex128_vector_in(self):
        a = self.np.array([1 + 2j, 3 - 4j], dtype=self.np.complex128)
        assert j("+/ {z}", {"z": a}) == 4 - 2j

    def test_complex128_is_borrowed_not_copied(self):
        a = self.np.array([1 + 1j, 2 + 2j], dtype=self.np.complex128)
        v = j("] {z}", {"z": a})
        a[0] = 9 + 9j
        assert v.tolist()[0] == 9 + 9j

    def test_a_rank_zero_complex_result_is_a_python_complex(self):
        v = j("3j4")
        assert isinstance(v, complex)
        assert v == 3 + 4j
        assert j("%: _4") == 2j

    def test_tolist_yields_python_complex(self):
        v = j("3j4 1j1")
        assert v.dtype == "complex"
        assert v.tolist() == [3 + 4j, 1 + 1j]
        assert repr(v) == "3j4 1j1"

    def test_a_python_complex_binds_as_data(self):
        assert j("{z} * {z}", {"z": 3 + 4j}) == -7 + 24j

    def test_round_trip_through_numpy(self):
        a = self.np.array([1 + 2j, 3 - 4j], dtype=self.np.complex128)
        assert j("+ {z}", {"z": a}).tolist() == list(self.np.conj(a))
        assert j("{z} * {z}", {"z": a}).tolist() == list(a * a)

    def test_shape_survives_a_complex_matrix(self):
        a = self.np.array([[1 + 1j, 2 + 2j], [3 + 3j, 4 + 4j]], dtype=self.np.complex128)
        v = j("] {z}", {"z": a})
        assert v.shape == (2, 2)
        assert v.tolist() == [[1 + 1j, 2 + 2j], [3 + 3j, 4 + 4j]]


class TestComplexArrow:
    @pytest.fixture(autouse=True)
    def _pa(self):
        self.pa = pytest.importorskip("pyarrow")
        self.np = pytest.importorskip("numpy")

    def test_result_exports_as_a_struct_of_re_and_im(self):
        out = self.pa.array(j("3j4 1j_1"))
        assert out.type == self.pa.struct(
            [
                self.pa.field("re", self.pa.float64(), nullable=False),
                self.pa.field("im", self.pa.float64(), nullable=False),
            ]
        )
        assert out.to_pylist() == [
            {"re": 3.0, "im": 4.0},
            {"re": 1.0, "im": -1.0},
        ]

    def test_a_struct_of_re_and_im_arrives_as_complex(self):
        arr = self.pa.array(j("3j4 1j_1"))
        assert j("+/ {z}", {"z": arr}) == 4 + 3j

    def test_round_trip_through_arrow(self):
        a = self.np.array([1 + 2j, 3 - 4j], dtype=self.np.complex128)
        arr = self.pa.array(j("] {z}", {"z": a}))
        assert j("] {z}", {"z": arr}).tolist() == list(a)

    def test_a_re_im_column_pair_in_a_table_is_one_complex_column(self):
        table = self.pa.table(
            {"x_re": [1.0, 3.0], "x_im": [2.0, -4.0]}
        )
        assert j("+/ {t}", {"t": table}) == 4 - 2j

    def test_an_unpaired_re_column_stays_a_float_column(self):
        table = self.pa.table({"x_re": [1.0, 3.0], "y": [2.0, 4.0]})
        assert j("+/ {t}", {"t": table}).tolist() == [4.0, 6.0]
