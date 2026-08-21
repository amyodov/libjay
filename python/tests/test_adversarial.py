"""Adversarial cases at the Python boundary.

Every test here is a way of getting it wrong: memory that is not laid out
the way it claims, values J has no room for, calls with the wrong names,
and requests larger than any machine will hold. What is pinned is the
answer the caller gets — a value or a refusal that says what to do next,
never a wrong number and never a dead process.
"""

import gc

import pytest

import jay
from jay import JayError, apl, j


class TestNumpyLayout:
    """Shapes and strides: what libjay reads, and what it refuses to read."""

    @pytest.fixture(autouse=True)
    def _np(self):
        self.np = pytest.importorskip("numpy")

    @pytest.mark.parametrize(
        "shape,expected",
        [((0,), [0]), ((0, 3), [0, 3]), ((3, 0), [3, 0]), ((0, 0), [0, 0])],
    )
    def test_empty_arrays_keep_their_shape(self, shape, expected):
        a = self.np.zeros(shape, dtype=self.np.int64)
        assert j("$ {x}", {"x": a}).tolist() == expected

    @pytest.mark.parametrize("dtype", ["int64", "float64", "bool"])
    def test_an_empty_array_sums_to_zero(self, dtype):
        a = self.np.array([], dtype=dtype)
        assert j("+/ {x}", {"x": a}) == 0

    @pytest.mark.parametrize(
        "value,expected", [(5, 6), (5.5, 6.5), (True, 2), (2**40, 2**40 + 1)]
    )
    def test_a_zero_d_array_is_a_scalar(self, value, expected):
        a = self.np.array(value)
        assert j("1 + {x}", {"x": a}) == expected
        assert j("$ {x}", {"x": a}).tolist() == []

    @pytest.mark.parametrize(
        "view",
        [
            lambda np: np.arange(10, dtype=np.int64)[::2],
            lambda np: np.arange(10, dtype=np.int64)[::-1],
            lambda np: np.arange(10, dtype=np.int64)[1:8:3],
            lambda np: np.arange(6, dtype=np.int64).reshape(2, 3).T,
            lambda np: np.arange(6, dtype=np.int64).reshape(2, 3)[:, :2],
            lambda np: np.arange(24, dtype=np.float64).reshape(2, 3, 4).transpose(1, 0, 2),
            lambda np: np.asfortranarray(np.arange(6, dtype=np.int64).reshape(2, 3)),
        ],
    )
    def test_a_non_contiguous_view_is_refused(self, view):
        a = view(self.np)
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a})
        message = str(e.value)
        assert "not contiguous" in message
        assert ".copy()" in message

    @pytest.mark.parametrize(
        "view,expected",
        [
            (lambda np: np.arange(10, dtype=np.int64)[::2], 20),
            (lambda np: np.arange(6, dtype=np.int64).reshape(2, 3).T, 15),
            (
                lambda np: np.asfortranarray(np.arange(6, dtype=np.int64).reshape(2, 3)),
                15,
            ),
        ],
    )
    def test_a_copy_of_the_refused_view_is_read(self, view, expected):
        a = self.np.ascontiguousarray(view(self.np))
        assert j("+/ , {x}", {"x": a}) == expected

    def test_fortran_order_reads_the_same_as_its_c_copy(self):
        c = self.np.arange(6, dtype=self.np.float64).reshape(2, 3)
        f = self.np.asfortranarray(c)
        assert j("+/ {x}", {"x": f.copy(order="C")}).tolist() == j(
            "+/ {x}", {"x": c}
        ).tolist()

    def test_a_one_column_fortran_array_is_contiguous_and_is_read(self):
        # An array with a single column is C-contiguous whatever numpy
        # calls its order, and numpy reports no strides for it.
        f = self.np.asfortranarray(self.np.arange(3, dtype=self.np.int64).reshape(3, 1))
        assert j("+/ {x}", {"x": f}).tolist() == [3]

    def test_a_transposed_vector_is_the_vector(self):
        a = self.np.arange(4, dtype=self.np.int64)
        assert j("+/ {x}", {"x": a.T}) == 6

    def test_an_empty_view_is_still_a_view(self):
        assert j("$ {x}", {"x": self.np.arange(0, dtype=self.np.int64)[::2]}).tolist() == [0]


class TestNumpyDtypes:
    @pytest.fixture(autouse=True)
    def _np(self):
        self.np = pytest.importorskip("numpy")

    @pytest.mark.parametrize(
        "dtype,value",
        [
            ("uint8", 2**8 - 1),
            ("uint16", 2**16 - 1),
            ("uint32", 2**32 - 1),
            ("uint64", 2**63 - 1),
            ("int8", -(2**7)),
            ("int16", -(2**15)),
            ("int32", -(2**31)),
            ("int64", -(2**63)),
        ],
    )
    def test_the_widest_value_of_each_width_crosses_whole(self, dtype, value):
        a = self.np.array([value], dtype=dtype)
        assert j("{x}", {"x": a}).tolist() == [value]

    @pytest.mark.parametrize("value", [2**63, 2**63 + 1, 2**64 - 1])
    def test_an_unsigned_value_past_i64_is_refused(self, value):
        a = self.np.array([value], dtype=self.np.uint64)
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a})
        message = str(e.value)
        assert str(value) in message
        assert "64-bit signed integer" in message

    def test_one_bad_value_refuses_the_whole_column(self):
        a = self.np.array([1, 2, 2**63], dtype=self.np.uint64)
        with pytest.raises(JayError, match="9223372036854775808"):
            j("+/ {x}", {"x": a})

    @pytest.mark.parametrize(
        "values,total", [([True, True, False], 2), ([False], 0), ([True] * 9, 9)]
    )
    def test_bool_arrays_arrive_as_booleans(self, values, total):
        a = self.np.array(values, dtype=bool)
        assert j("+/ {x}", {"x": a}) == total
        assert j("{x}", {"x": a}).dtype == "boolean"

    @pytest.mark.parametrize("value", [1.5, 0.1, 3.4028235e38, -0.0])
    def test_float32_widens_to_the_double_it_names(self, value):
        a = self.np.array([value], dtype=self.np.float32)
        assert j("{x}", {"x": a}).tolist() == [float(self.np.float32(value))]

    def test_an_object_array_names_the_problem(self):
        a = self.np.array([1, 2, 3], dtype=object)
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": a})
        message = str(e.value)
        assert "object" in message
        assert "Python objects" in message
        assert "astype" in message

    def test_an_object_array_of_strings_is_refused_the_same_way(self):
        a = self.np.array(["a", "b"], dtype=object)
        with pytest.raises(JayError, match="object"):
            j("+/ {x}", {"x": a})

    @pytest.mark.parametrize(
        "maker",
        [
            lambda np: np.array([1.0, 2.0], dtype=np.float16),
            lambda np: np.array(["ab", "cd"]),
            lambda np: np.array(["2020-01-01"], dtype="datetime64[s]"),
            lambda np: np.zeros(2, dtype=[("a", "<i8"), ("b", "<f8")]),
        ],
    )
    def test_an_unreadable_dtype_says_what_is_supported(self, maker):
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": maker(self.np)})
        message = str(e.value)
        assert "not supported yet" in message
        assert "supported: bool" in message

    def test_a_byte_swapped_array_is_refused(self):
        a = self.np.array([1, 2], dtype=">i8" if self.np.little_endian else "<i8")
        with pytest.raises(JayError, match="byte order"):
            j("+/ {x}", {"x": a})

    def test_a_borrowed_block_outlives_the_name_that_held_it(self):
        a = self.np.arange(1000, dtype=self.np.float64)
        held = j("] {x}", {"x": a})
        del a
        gc.collect()
        assert held.tolist()[-1] == 999.0


class TestArrowChunks:
    @pytest.fixture(autouse=True)
    def _pa(self):
        self.pa = pytest.importorskip("pyarrow")

    @pytest.mark.parametrize(
        "chunks,total",
        [
            ([], 0),
            ([[]], 0),
            ([[1, 2, 3]], 6),
            ([[1, 2], [3, 4]], 10),
            ([[1], [2], [3], [4], [5]], 15),
            ([[1, 2], [], [3]], 6),
            ([[], [], [7]], 7),
        ],
    )
    def test_a_chunked_column_reads_as_one_vector(self, chunks, total):
        column = self.pa.chunked_array(chunks, type=self.pa.int64())
        assert j("+/ {x}", {"x": column}) == total
        assert j("# {x}", {"x": column}) == sum(len(c) for c in chunks)

    @pytest.mark.parametrize("type_name", ["float64", "int32", "bool_", "uint8"])
    def test_chunking_does_not_change_the_answer(self, type_name):
        ty = getattr(self.pa, type_name)()
        values = [True, False, True, True] if type_name == "bool_" else [1, 0, 1, 1]
        whole = self.pa.chunked_array([values], type=ty)
        split = self.pa.chunked_array([values[:1], values[1:3], values[3:]], type=ty)
        assert j("+/ {x}", {"x": whole}) == j("+/ {x}", {"x": split})

    def test_a_null_in_a_late_chunk_is_still_found(self):
        column = self.pa.chunked_array([[1, 2], [3, None]], type=self.pa.int64())
        with pytest.raises(JayError, match="null"):
            j("+/ {x}", {"x": column})


class TestArrowNulls:
    """Nulls are refused permanently, and the wording says so: J has no
    missing value, so this is not a promise to add one."""

    @pytest.fixture(autouse=True)
    def _pa(self):
        self.pa = pytest.importorskip("pyarrow")

    def types(self):
        pa = self.pa
        return {
            "int8": pa.int8(),
            "int16": pa.int16(),
            "int32": pa.int32(),
            "int64": pa.int64(),
            "uint8": pa.uint8(),
            "uint16": pa.uint16(),
            "uint32": pa.uint32(),
            "uint64": pa.uint64(),
            "float32": pa.float32(),
            "float64": pa.float64(),
            "bool": pa.bool_(),
            "date32": pa.date32(),
            "date64": pa.date64(),
            "timestamp": pa.timestamp("us"),
            "duration": pa.duration("s"),
            "time32": pa.time32("s"),
            "time64": pa.time64("us"),
        }

    @pytest.mark.parametrize(
        "type_name",
        [
            "int8", "int16", "int32", "int64",
            "uint8", "uint16", "uint32", "uint64",
            "float32", "float64", "bool",
            "date32", "date64", "timestamp", "duration", "time32", "time64",
        ],
    )
    def test_a_null_of_any_type_is_refused_permanently(self, type_name):
        column = self.pa.array([None, None], type=self.types()[type_name])
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": column})
        message = str(e.value)
        assert "no representation for missing values" in message
        assert "fill or filter" in message
        # A permanent absence, not a promise: nothing here says "yet".
        assert "yet" not in message

    def test_the_message_counts_the_nulls_and_names_the_column(self):
        table = self.pa.table({"close": self.pa.array([1.0, None, None, 4.0])})
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": table})
        message = str(e.value)
        assert "column 'close'" in message
        assert "2 null(s)" in message

    def test_an_all_null_column_is_refused_as_a_null_not_as_a_type(self):
        # Arrow's own Null type carries no values at all; the refusal is
        # about the missing values, not about an unimplemented type.
        table = self.pa.table({"a": [1, 2], "b": [None, None]})
        with pytest.raises(JayError) as e:
            j("+/ {x}", {"x": table})
        message = str(e.value)
        assert "column 'b'" in message
        assert "no representation for missing values" in message
        assert "yet" not in message

    def test_a_null_in_one_half_of_a_complex_pair_is_refused(self):
        table = self.pa.table({"x_re": [1.0, None], "x_im": [2.0, 3.0]})
        with pytest.raises(JayError, match="null"):
            j("+/ {x}", {"x": table})


class TestArrowTables:
    @pytest.fixture(autouse=True)
    def _pa(self):
        self.pa = pytest.importorskip("pyarrow")

    def test_a_table_of_no_rows_is_a_zero_by_n_matrix(self):
        empty = self.pa.array([], type=self.pa.int64())
        table = self.pa.table({"a": empty, "b": empty})
        assert j("$ {t}", {"t": table}).tolist() == [0, 2]
        assert j("+/ {t}", {"t": table}).tolist() == [0, 0]

    def test_a_table_of_no_columns_is_empty(self):
        assert j("$ {t}", {"t": self.pa.table({})}).tolist() == [0, 0]

    def test_a_table_of_one_row_is_still_a_matrix(self):
        table = self.pa.table({"a": [1], "b": [2]})
        assert j("$ {t}", {"t": table}).tolist() == [1, 2]
        assert j("+/ {t}", {"t": table}).tolist() == [1, 2]
        assert j('+/"1 {t}', {"t": table}).tolist() == [3]

    def test_a_single_column_table_of_one_row_is_a_one_item_vector(self):
        assert j("$ {t}", {"t": self.pa.table({"a": [7]})}).tolist() == [1]

    @pytest.mark.parametrize(
        "names", [["a", "a"], ["", ""], ["a b", "a b"]]
    )
    def test_duplicate_column_names_are_two_columns(self, names):
        table = self.pa.table(
            [self.pa.array([1, 2]), self.pa.array([3, 4])], names=names
        )
        assert j("+/ {t}", {"t": table}).tolist() == [3, 7]

    @pytest.mark.parametrize("name", ["a b", "1x", "", "x-y", "π", "a.b", "0"])
    def test_a_column_name_need_not_be_an_identifier(self, name):
        table = self.pa.table({name: [1, 2, 3]})
        assert j("+/ {t}", {"t": table}) == 6

    @pytest.mark.parametrize("name", ["a b", "", "x-y"])
    def test_a_refusal_still_quotes_the_odd_name(self, name):
        table = self.pa.table({name: [1, None]})
        with pytest.raises(JayError) as e:
            j("+/ {t}", {"t": table})
        assert (f"column '{name}'" if name else "the input") in str(e.value)

    def test_a_string_column_names_itself_in_the_refusal(self):
        table = self.pa.table({"sym": ["a", "b"], "n": [1, 2]})
        with pytest.raises(JayError) as e:
            j("+/ {t}", {"t": table})
        message = str(e.value)
        assert "column 'sym'" in message
        assert "string column" in message

    def test_a_table_of_string_columns_only_is_refused_too(self):
        table = self.pa.table({"a": ["x"], "b": ["y"]})
        with pytest.raises(JayError, match="string column"):
            j("+/ {t}", {"t": table})

    def test_a_record_batch_reads_like_a_table(self):
        batch = self.pa.record_batch({"a": [1, 2], "b": [3, 4]})
        assert j("+/ {t}", {"t": batch}).tolist() == [3, 7]

    def test_a_sliced_table_reads_the_slice(self):
        table = self.pa.table({"a": [1, 2, 3], "b": [4, 5, 6]}).slice(1, 2)
        assert j("] {t}", {"t": table}).tolist() == [[2, 5], [3, 6]]


class TestPandasFrames:
    @pytest.fixture(autouse=True)
    def _pd(self):
        self.pd = pytest.importorskip("pandas")
        if not hasattr(self.pd.DataFrame, "__arrow_c_stream__"):
            pytest.skip("pandas without the Arrow stream interface")

    def test_a_frame_of_no_rows_is_a_zero_by_n_matrix(self):
        df = self.pd.DataFrame(
            {"a": self.pd.Series([], dtype="int64"), "b": self.pd.Series([], dtype="int64")}
        )
        assert j("$ {df}", {"df": df}).tolist() == [0, 2]

    def test_a_frame_of_no_columns_is_empty(self):
        assert j("$ {df}", {"df": self.pd.DataFrame()}).tolist() == [0, 0]

    def test_a_frame_of_one_row_is_a_matrix(self):
        df = self.pd.DataFrame({"a": [1.0], "b": [2.0]})
        assert j("$ {df}", {"df": df}).tolist() == [1, 2]
        assert j('+/"1 {df}', {"df": df}).tolist() == [3.0]

    @pytest.mark.parametrize("columns", [["a b", "3"], [0, 1], ["", " "]])
    def test_a_column_name_need_not_be_an_identifier(self, columns):
        df = self.pd.DataFrame([[1, 3], [2, 4]], columns=columns)
        assert j("+/ {df}", {"df": df}).tolist() == [3, 7]

    def test_a_missing_value_is_refused_by_name(self):
        df = self.pd.DataFrame({"close": self.pd.array([1, None], dtype="Int64")})
        with pytest.raises(JayError) as e:
            j("+/ {df}", {"df": df})
        message = str(e.value)
        assert "column 'close'" in message
        assert "no representation for missing values" in message

    def test_a_pandas_nan_arrives_as_a_null_and_is_refused(self):
        # J has a NaN of its own, but pandas exports a float NaN through
        # Arrow as a null, and a null is what the boundary sees.
        df = self.pd.DataFrame({"a": [1.0, float("nan")]})
        with pytest.raises(JayError, match="null"):
            j("{df}", {"df": df})

    def test_a_numpy_nan_is_a_number(self):
        np = pytest.importorskip("numpy")
        a = np.array([1.0, float("nan")])
        value = j("{x}", {"x": a}).tolist()[1]
        assert value != value

    def test_a_string_column_names_itself(self):
        df = self.pd.DataFrame({"sym": ["a"], "n": [1]})
        with pytest.raises(JayError, match="column 'sym'"):
            j("+/ {df}", {"df": df})


class TestCallMisuse:
    """Names and values at the call: what is missing, what is extra, and
    what is not a value at all."""

    @pytest.mark.parametrize("source", ["", " ", "\n", "   \n  "])
    def test_an_empty_expression_compiles_and_yields_nothing(self, source):
        kernel = jay.j.compile(source)
        assert kernel.params == []
        assert kernel() is None
        assert j(source) is None
        assert apl(source) is None

    def test_an_empty_expression_explains_itself(self):
        assert "source" in jay.j.compile("").explain()

    @pytest.mark.parametrize("where", ["bind", "call", "explain"])
    def test_an_unknown_name_is_refused_wherever_it_is_offered(self, where):
        kernel = jay.j.compile("{x} + 1", {"x": 1})
        with pytest.raises(JayError) as e:
            {
                "bind": lambda: kernel.bind({"y": 2}),
                "call": lambda: kernel({"y": 2}),
                "explain": lambda: kernel.explain({"y": 2}),
            }[where]()
        message = str(e.value)
        assert "unknown parameter(s): y" in message
        assert "this expression has: x" in message

    def test_an_unknown_name_against_a_parameterless_expression(self):
        with pytest.raises(JayError, match="has: none"):
            jay.j.compile("1 + 1").bind({"x": 1})

    @pytest.mark.parametrize(
        "source,bound,missing",
        [
            ("{x} + 1", {}, "x"),
            ("{x} + {y}", {"x": 1}, "y"),
            ("{a} + {b} + {c}", {"b": 1}, "a, c"),
        ],
    )
    def test_a_missing_value_names_the_parameters(self, source, bound, missing):
        kernel = jay.j.compile(source).bind(bound) if bound else jay.j.compile(source)
        with pytest.raises(JayError) as e:
            kernel()
        assert f"missing value(s) for parameter(s): {missing}" in str(e.value)

    def test_a_missing_value_leaves_explain_describing_the_structure(self):
        text = jay.j.compile("{x} + 1").explain()
        assert "none supplied" in text
        assert "parameters: x" in text

    @pytest.mark.parametrize("where", ["bind", "call"])
    def test_none_is_not_a_value(self, where):
        kernel = jay.j.compile("{x} + 1")
        with pytest.raises(TypeError) as e:
            (kernel.bind({"x": None})() if where == "bind" else kernel({"x": None}))
        assert "NoneType" in str(e.value)

    def test_a_value_bound_later_replaces_the_one_bound_earlier(self):
        kernel = jay.j.compile("+/ {x}", {"x": [1, 2, 3]})
        assert kernel() == 6
        assert kernel.bind({"x": [10, 20]})() == 30
        assert kernel({"x": [100]}) == 100
        # Neither rebinding disturbed the kernel's own default.
        assert kernel() == 6

    @pytest.mark.parametrize("value", [object(), {"a": 1}, set(), b"bytes"])
    def test_an_unsupported_value_says_what_is_supported(self, value):
        with pytest.raises(TypeError) as e:
            j("{x}", {"x": value})
        assert "cannot pass a" in str(e.value)

    def test_a_source_that_is_not_a_string_is_refused(self):
        with pytest.raises(TypeError, match="expected str or Template"):
            jay.j.compile(5)


class TestTemplateMisuse:
    @pytest.fixture(autouse=True)
    def _templates(self):
        templatelib = pytest.importorskip("string.templatelib")
        self.Template = templatelib.Template
        self.Interpolation = templatelib.Interpolation

    def template(self, first, second):
        """``t"{x} + {x}"`` with the two holes carrying these values."""
        return self.Template(
            "", self.Interpolation(first, "x"), " + ", self.Interpolation(second, "x"), ""
        )

    def test_a_name_used_twice_is_one_parameter(self):
        kernel = jay.j.compile(self.template(3, 3))
        assert kernel.params == ["x"]
        assert kernel() == 6

    def test_rebinding_that_name_changes_both_of_its_places(self):
        kernel = jay.j.compile(self.template([1, 2], [1, 2]))
        assert kernel().tolist() == [2, 4]
        assert kernel.bind({"x": [10, 20]})().tolist() == [20, 40]

    def test_the_string_form_repeats_a_hole_the_same_way(self):
        kernel = jay.j.compile("{x} + {x}", {"x": 3})
        assert kernel.params == ["x"]
        assert kernel() == 6

    def test_an_interpolation_that_is_not_a_name_is_refused(self):
        t = self.Template("", self.Interpolation(3, "x + 1"), "")
        with pytest.raises(TypeError) as e:
            jay.j.compile(t)
        assert "plain identifier" in str(e.value)


class TestExplainPaths:
    """explain() answers on every path a call can take, including the ones
    that end in an error."""

    def test_structure_only_when_a_value_is_missing(self):
        assert "none supplied" in jay.j.compile("+/ {x}").explain()

    def test_shapes_when_every_value_is_there(self):
        text = jay.j.compile("+/ {x}", {"x": [1, 2, 3]}).explain()
        assert "3 $ integer" in text

    def test_a_failing_run_is_explained_up_to_the_failure(self):
        text = jay.j.compile("{x} + {y}", {"x": [1, 2], "y": [1, 2, 3]}).explain()
        assert "the run stopped here" in text
        assert "length error" in text

    def test_a_value_that_cannot_be_read_stops_explain_the_way_it_stops_a_call(self):
        np = pytest.importorskip("numpy")
        kernel = jay.j.compile("+/ {x}")
        bad = np.arange(6, dtype=np.int64).reshape(2, 3).T
        with pytest.raises(JayError, match="not contiguous"):
            kernel.explain({"x": bad})
        with pytest.raises(JayError, match="not contiguous"):
            kernel({"x": bad})

    @pytest.mark.parametrize(
        "source,data",
        [
            ("+/ {x}", {"x": [1, 2, 3]}),
            ("{x} , {x}", {"x": "abc"}),
            ("<{x}", {"x": [[1, 2], [3]]}),
            ("{x} % 0", {"x": [1.0]}),
        ],
    )
    def test_explain_answers_rather_than_raising(self, source, data):
        assert jay.j.compile(source, data).explain()

    def test_explain_of_an_apl_expression_is_apl(self):
        text = jay.apl.compile("+/{x}", {"x": [1, 2, 3]}).explain()
        assert "+/{x}" in text


class TestCompileErrors:
    @pytest.mark.parametrize(
        "source,fragment",
        [
            ("foo 1", "undefined name: foo"),
            ("1 2 3 + 1 2", "left shape 3, right shape 2"),
            ("'a' + 1", "character and numeric"),
            ("{x} + 1", "unknown parameter"),
        ],
    )
    def test_an_error_points_into_the_source(self, source, fragment):
        with pytest.raises(JayError) as e:
            j(source, {"y": 1} if "{x}" in source else None)
        assert fragment in str(e.value)

    def test_a_shape_error_shows_both_shapes(self):
        with pytest.raises(JayError) as e:
            j("1 2 3 + 1 2")
        message = str(e.value)
        assert "left shape 3" in message
        assert "right shape 2" in message
        assert "^" in message


class TestResourceGuards:
    """A request larger than any machine holds comes back as an error, at
    once and without allocating."""

    @pytest.mark.parametrize(
        "source",
        [
            "1e12 $ 0",
            "1000000000000 $ 0",
            "1000000 1000000 $ 0",
            "4294967296 4294967296 $ 0",
            "i. 10000000000",
            "i. 1000000 1000000",
            "1000000000000 {. 1 2 3",
            "1000000000000 # 1",
            "1000000000000 # ,: i. 0",
        ],
    )
    def test_a_giant_j_request_is_refused(self, source):
        with pytest.raises(JayError) as e:
            j(source)
        message = str(e.value)
        assert "limit error" in message
        assert "ceiling" in message

    @pytest.mark.parametrize(
        "source", ["1000000000000⍴0", "⍳10000000000", "⍳1000000 1000000"]
    )
    def test_a_giant_apl_request_is_refused(self, source):
        with pytest.raises(JayError, match="ceiling"):
            apl(source)

    def test_the_refusal_names_the_count_that_was_asked_for(self):
        with pytest.raises(JayError) as e:
            j("1000000 1000000 $ 0")
        message = str(e.value)
        assert "1000000000000 elements" in message
        assert "1000000 1000000" in message

    def test_a_product_that_would_wrap_is_refused_rather_than_wrapping(self):
        # 2^32 * 2^32 is zero in machine arithmetic; an unguarded product
        # would make this an empty array of an enormous shape.
        with pytest.raises(JayError, match="18446744073709551616"):
            j("4294967296 4294967296 $ 0")

    @pytest.mark.parametrize(
        "shape,items",
        [("0 1000000000000 $ 0", 0), ("1000000000000 0 $ 0", 0)],
    )
    def test_an_empty_axis_is_not_a_giant_request(self, shape, items):
        assert j("# , " + shape) == items

    @pytest.mark.parametrize(
        "source,expected",
        [("2 3 $ i. 6", [[0, 1, 2], [3, 4, 5]]), ("5 {. 1 2 3", [1, 2, 3, 0, 0])],
    )
    def test_ordinary_shapes_are_untouched(self, source, expected):
        assert j(source).tolist() == expected


class TestRecursionGuard:
    """Runaway recursion stops as a diagnostic, not as a dead process."""

    RUNAWAY = [
        # J: `$:` is the definition it stands in.
        ("j", "f =. 3 : 'if. y = 0 do. 0 else. $: y - 1 end.'\nf 1000"),
        # J: a definition that calls itself by name.
        ("j", "f =. 3 : '1 + f y'\nf 1"),
        # APL: `∇` is the same self-reference.
        ("apl", "f←{⍵=0:0 ⋄ ∇⍵-1}\nf 1000"),
    ]

    @pytest.mark.parametrize("lang,source", RUNAWAY)
    def test_recursion_past_the_guard_raises(self, lang, source):
        with pytest.raises(JayError) as e:
            {"j": j, "apl": apl}[lang](source)
        message = str(e.value)
        assert "deep" in message
        assert "^" in message

    @pytest.mark.parametrize(
        "lang,source,expected",
        [
            ("j", "f =. 3 : 'if. y = 0 do. 0 else. $: y - 1 end.'\nf 5", 0),
            ("apl", "f←{⍵=0:0 ⋄ ∇⍵-1}\nf 5", 0),
        ],
    )
    def test_recursion_inside_the_guard_still_answers(self, lang, source, expected):
        assert {"j": j, "apl": apl}[lang](source) == expected

    def test_the_guard_does_not_leak_between_calls(self):
        source = "f =. 3 : 'if. y = 0 do. 0 else. $: y - 1 end.'\nf 60"
        kernel = jay.j.compile(source)
        assert kernel() == 0
        assert kernel() == 0
