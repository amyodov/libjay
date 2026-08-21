import math
from fractions import Fraction

import pytest

import jay
from jay import JayError, apl, j


class TestOneShot:
    def test_scalar_arithmetic(self):
        assert j("2 + 2") == 4

    def test_division_is_float(self):
        assert j("10 % 4") == 2.5

    def test_vector_result(self):
        assert j("1 2 3 * 10").tolist() == [10, 20, 30]

    def test_matrix_result(self):
        v = j("i. 2 3")
        assert v.shape == (2, 3)
        assert v.tolist() == [[0, 1, 2], [3, 4, 5]]

    def test_fork_mean(self):
        assert j("(+/ % #) 3 1 4 1 5") == 2.8

    def test_string_result_is_str(self):
        assert j("'hello'") == "hello"

    def test_apl(self):
        assert apl("2+2") == 4
        assert apl("+/2 3⍴⍳6").tolist() == [6, 15]
        assert apl("+⌿2 3⍴⍳6").tolist() == [5, 7, 9]

    def test_assignment_sequence(self):
        assert j("x =. 5\nx * 2") == 10
        assert j("x =. 5") is None


class TestDefinitions:
    def test_j_explicit_definition(self):
        assert j("f =. 3 : 'y*2'\nf 21") == 42

    def test_j_dyadic_definition(self):
        assert j("f =. 4 : 'x + y'\n2 f 3") == 5

    def test_j_direct_definition(self):
        assert j("f =. {{ y * 3 }}\nf 7") == 21

    def test_j_multi_line_definition(self):
        assert j("f =. 3 : 0\nt =. y * 2\nt + 1\n)\nf 5") == 11

    def test_j_control_words(self):
        src = (
            "fac =. 3 : 'if. y <: 1 do. 1 else. y * fac y - 1 end.'\n"
            "fac 5"
        )
        assert j(src) == 120

    def test_j_definition_over_bound_data(self):
        assert j("f =. 3 : '+/ y'\nf {x}", {"x": [1, 2, 3, 4]}) == 10

    def test_apl_dfn(self):
        assert apl("F←{⍵×2} ⋄ F 21") == 42

    def test_apl_tradfn_with_control_structure(self):
        src = "∇Z←H R\n:If R>3\nZ←99\n:Else\nZ←7\n:EndIf\n∇\nH 5"
        assert apl(src) == 99

    def test_apl_indexed_assignment(self):
        assert apl("A←1 2 3 4 ⋄ A[2]←99 ⋄ A").tolist() == [1, 99, 3, 4]

    def test_runaway_recursion_is_an_error_not_a_crash(self):
        with pytest.raises(JayError, match="deep"):
            j("f =. 3 : '$: y'\nf 1")


class TestData:
    def test_dict_binding(self):
        assert j("+/ {x}", {"x": [1, 2, 3]}) == 6

    def test_nested_lists(self):
        got = j("+/ {m}", {"m": [[1, 2], [10, 20]]})
        assert got.tolist() == [11, 22]

    def test_floats(self):
        assert j("+/ {x} * {y}", {"x": [0.5, 0.5], "y": [3.0, 5.0]}) == 4.0

    def test_repeated_param(self):
        k = j.compile("{x} + {x}")
        assert k.params == ["x"]
        assert k({"x": 21}) == 42

    def test_missing_param(self):
        k = j.compile("{a} + {b}")
        with pytest.raises(JayError, match="missing"):
            k({"a": 1})

    def test_unknown_param(self):
        k = j.compile("{a} + 1")
        with pytest.raises(JayError, match="unknown"):
            k({"b": 2})

    def test_ragged_input_becomes_boxes(self):
        # A list whose items do not share a shape is a boxed vector, not
        # an error: the dense path is tried first and only then this one.
        v = j("{m}", {"m": [[1, 2], [3]]})
        assert v.dtype == "boxed"
        assert v.tolist() == [[1, 2], [3]]
        # Arithmetic still needs the boxes opened.
        with pytest.raises(JayError, match="boxed"):
            j("+/ {m}", {"m": [[1, 2], [3]]})

    def test_list_of_strings_is_a_boxed_vector(self):
        assert j("# &.> {names}", {"names": ["ab", "cde"]}).tolist() == [2, 3]
        assert apl("≢¨{names}", {"names": ["ab", "cde"]}).tolist() == [2, 3]
        v = j("{names}", {"names": ["ab", "cde"]})
        assert v.dtype == "boxed"
        assert v.tolist() == ["ab", "cde"]
        assert j("; {names}", {"names": ["ab", "cde"]}) == "abcde"


class TestBoxes:
    def test_boxed_result_converts_to_nested_python_data(self):
        v = j("1;2 3;'abc'")
        assert v.dtype == "boxed"
        assert v.shape == (3,)
        assert v.depth == 2
        assert v.tolist() == [1, [2, 3], "abc"]

    def test_a_box_hands_back_what_it_holds(self):
        # A rank-0 box is its contents at whatever shape they have.
        assert j("<'abc'") == "abc"
        assert j("< 1 2 3").tolist() == [1, 2, 3]
        assert j("< 5") == 5

    def test_repr_is_the_j_box_drawing(self):
        assert repr(j("1;2 3")) == "+-+---+\n|1|2 3|\n+-+---+"

    def test_apl_nesting(self):
        assert apl("≡(1 2)(3 4)") == 2
        assert apl("∊(1 2)(3 4 5)").tolist() == [1, 2, 3, 4, 5]
        assert apl("(1 2)(3 4)").tolist() == [[1, 2], [3, 4]]
        assert apl("⊂1 2 3").tolist() == [1, 2, 3]

    def test_depth_of_simple_values(self):
        assert j("5").__class__ is int
        assert j("1 2 3").depth == 1
        assert j("i. 2 3").depth == 1


class TestExactNumbers:
    """J's extended integers and rationals across the Python boundary."""

    def test_a_big_result_is_an_exact_python_int(self):
        assert j("! 30x") == 265252859812191058636308480000000
        assert j("2 ^ 100x") == 2**100
        assert j("*/ >: i. 25x") == math.factorial(25)

    def test_a_big_python_int_arrives_exact(self):
        big = 2**100
        assert j("{n} + 1", {"n": big}) == big + 1
        assert j("{n} = {n}", {"n": big}) == 1

    def test_a_rational_result_is_a_fraction(self):
        v = j("1r2 + 1r3")
        assert v == Fraction(5, 6)
        assert isinstance(v, Fraction)

    def test_a_fraction_argument_arrives_as_a_rational(self):
        assert j("{a} + {b}", {"a": Fraction(1, 2), "b": Fraction(1, 3)}) == Fraction(5, 6)
        assert j("{a} * 6", {"a": Fraction(1, 2)}) == 3

    def test_a_vector_of_exact_values_converts_element_by_element(self):
        assert j("1 2 3x * {n}", {"n": 10**21}).tolist() == [
            10**21,
            2 * 10**21,
            3 * 10**21,
        ]
        assert j("1r2 1r3").tolist() == [Fraction(1, 2), Fraction(1, 3)]

    def test_dtype_names(self):
        assert j("1 2 3x").dtype == "extended"
        assert j("1r2 1r3").dtype == "rational"

    def test_arrow_has_no_carrier_for_them(self):
        pa = pytest.importorskip("pyarrow")
        with pytest.raises(JayError, match="no carrier"):
            pa.array(j("1 2 3x"))
        # The conversion back to machine numbers is the way out.
        assert pa.array(j("_1 x: 1 2 3x")).to_pylist() == [1, 2, 3]


class TestKernel:
    def test_bind_returns_new_kernel(self):
        k = j.compile("{x} * {y}")
        k2 = k.bind({"x": 3})
        assert k2 is not k
        assert k2({"y": 5}) == 15
        # The original is untouched.
        with pytest.raises(JayError, match="missing"):
            k({"y": 5})

    def test_override_cascade(self):
        k = j.compile("{x} + {y}", {"x": 1, "y": 10})
        assert k() == 11
        k2 = k.bind({"y": 20})
        assert k2() == 21
        assert k2({"x": 100}) == 120
        assert k2() == 21  # call-time override does not stick


class TestOutput:
    def test_echo_writes_stdout(self, capsys):
        j("echo 'hi'")
        assert capsys.readouterr().out == "hi\n"

    def test_quad_write(self, capsys):
        assert apl("⎕←2+2") is None
        assert capsys.readouterr().out == "4\n"

    def test_quote_quad_write_does_not_end_the_line(self, capsys):
        assert apl("⍞←'ab' ⋄ ⍞←'cd'") is None
        assert capsys.readouterr().out == "abcd"

    def test_j_write_foreign(self, capsys):
        assert j("'abc' 1!:2 ]2") == "abc"
        assert capsys.readouterr().out == "abc\n"


def _lines(*lines):
    """An input source that hands out these lines and then ends."""
    it = iter(lines)
    return lambda: next(it, None)


class TestInput:
    def test_character_input(self):
        assert apl("⍞", input=_lines("hello")) == "hello"

    def test_each_read_takes_the_next_line(self):
        assert apl("a←⍞ ⋄ b←⍞ ⋄ a,b", input=_lines("one", "two")) == "onetwo"

    def test_evaluated_input(self):
        assert apl("⎕", input=_lines("2+2")) == 4
        assert apl("1+⎕", input=_lines("⍳3")).tolist() == [2, 3, 4]

    def test_evaluated_input_sees_the_programs_names(self):
        assert apl("x←10 ⋄ ⎕", input=_lines("x×2")) == 20

    def test_j_read_foreign(self):
        assert j("1!:1 ]1", input=_lines("a line")) == "a line"
        assert j("1!:1 [1", input=_lines("a line")) == "a line"

    def test_end_of_input_is_an_error(self):
        with pytest.raises(JayError, match="the input has ended"):
            apl("⍞", input=_lines())
        with pytest.raises(JayError, match="the input has ended"):
            apl("a←⍞ ⋄ ⍞", input=_lines("only"))

    def test_no_input_source_is_a_different_error(self):
        with pytest.raises(JayError, match="no input source attached"):
            apl("⍞", input=None)

    def test_a_kernel_takes_input_per_call(self):
        k = apl.compile("⍞")
        assert k(input=_lines("first")) == "first"
        assert k(input=_lines("second")) == "second"

    def test_the_readers_own_failure_is_raised(self):
        def broken():
            raise RuntimeError("no reading today")

        with pytest.raises(RuntimeError, match="no reading today"):
            apl("⍞", input=broken)

    def test_stdin_is_the_default(self, monkeypatch):
        import io

        monkeypatch.setattr("sys.stdin", io.StringIO("piped line\n"))
        assert apl("⍞") == "piped line"

    def test_reading_and_writing_meet(self, capsys):
        assert j("(1!:1 ]1) 1!:2 ]2", input=_lines("through")) == "through"
        assert capsys.readouterr().out == "through\n"


class TestSandbox:
    def test_file_foreigns_are_closed(self):
        for src in ["1!:1 <'/etc/hosts'", "1!:21 <'f'", "2!:5 <'HOME'", "6!:0 ''"]:
            with pytest.raises(JayError, match="closed by the sandbox"):
                j(src)

    def test_system_names_that_reach_outside_are_closed(self):
        for src in ["⎕TS", "⎕AI", "⎕FIO"]:
            with pytest.raises(JayError, match="closed by the sandbox"):
                apl(src)

    def test_threads_are_closed(self):
        with pytest.raises(JayError, match="closed by the sandbox"):
            j("+ T. 1")

    def test_a_foreign_that_only_computes_is_a_promise(self):
        with pytest.raises(JayError, match="not supported yet"):
            j("9!:18 ''")

    def test_the_type_foreign(self):
        assert j("3!:0 (1.5)") == 8
        assert j("3!:0 'a'") == 2
        assert j("3!:0 (i.5)") == 4


class TestDialect:
    def test_index_origin(self):
        assert apl("⍳3").tolist() == [1, 2, 3]
        assert apl.compile("⍳3", index_origin=0)().tolist() == [0, 1, 2]

    def test_create_compiler(self):
        from jay.lang import APL

        c = APL.create_compiler(APL.Dialect(index_base=0))
        assert c("⍳3").tolist() == [0, 1, 2]

    def test_the_preset_is_the_default(self):
        from jay.lang import APL

        assert APL.Dialect.gnu == APL.Dialect()

    def test_comparison_tolerance(self):
        from jay.lang import APL

        assert apl("⎕CT") == pytest.approx(1e-13)
        c = APL.create_compiler(APL.Dialect(comparison_tolerance=1e-10))
        assert c("⎕CT") == pytest.approx(1e-10)
        # The setting is what comparisons use, not a number to read back.
        assert c("1=1+1e¯11") == 1
        assert apl("1=1+1e¯11") == 0

    def test_another_dialects_reading_is_refused(self):
        from jay.lang import APL

        # libjay implements one APL; asking for the other line's reading
        # of a divergence is a gap, said out loud.
        for setting in [
            {"nested_model": "grounded"},
            {"first_disclose": "up-is-mix"},
            {"index_form": "axis-vectors"},
            {"dfn_result": "first-non-assignment"},
            {"default_arg": "lazy"},
            {"complex_order": "magnitude-then-angle"},
        ]:
            c = APL.create_compiler(APL.Dialect(**setting))
            with pytest.raises(JayError, match="not supported yet"):
                c.compile("1 2 3")

    def test_trains_are_an_extension_that_can_be_turned_off(self):
        from jay.lang import APL

        # GNU APL has no trains and no function assignment; libjay ships
        # both on, and the strict reading is a setting away.
        assert APL.Dialect().trains is True
        assert apl("(+/÷≢)1 2 3 4") == 2.5
        assert apl("MEAN←+/÷≢\nMEAN 1 2 3 4") == 2.5
        strict = APL.create_compiler(APL.Dialect(trains=False))
        with pytest.raises(JayError):
            strict.compile("(+/÷≢)1 2 3 4")
        # Grouping a single function is not the extension, and stays.
        assert strict("(+)/1 2 3") == 6

    def test_an_unknown_setting_value_is_named(self):
        from jay.lang import APL

        c = APL.create_compiler(APL.Dialect(index_form="sideways"))
        with pytest.raises(JayError, match="unknown index_form"):
            c.compile("1 2 3")


class TestErrors:
    def test_error_points_into_source(self):
        with pytest.raises(JayError) as e:
            j("1 2 + 1 2 3")
        assert "^" in str(e.value)

    def test_not_yet_wording(self):
        with pytest.raises(JayError, match="not supported yet"):
            j("/: 1;2")

    def test_parse_error(self):
        with pytest.raises(JayError):
            j.compile("2 ]: 3")


class TestModuleShape:
    def test_re_module_shape(self):
        # Callable singletons with .compile, like re.match/re.compile.
        assert callable(j) and callable(j.compile)
        assert callable(apl)
        assert jay.compile("2+2", lang="j")() == 4


class TestNamedVerbs:
    def test_a_named_verb_applies_later(self):
        assert j("mean =. +/ % #\nmean 1 2 3 4") == 2.5

    def test_a_named_verb_is_a_verb_in_a_train(self):
        assert j("mean =. +/ % #\n(mean - {.) 1 2 3 4") == 1.5

    def test_naming_a_verb_yields_nothing(self):
        assert j("1 + 1\nmean =. +/ % #") is None

    def test_apl_function_assignment_names_a_function(self):
        assert apl("F←+/\nF 1 2 3") == 6
        assert apl("F←+/") is None

    def test_a_named_apl_function_is_a_function_in_a_train(self):
        assert apl("S←+/\nM←S÷≢\nM 1 2 3 4") == 2.5

    def test_j_names_an_adverb(self):
        assert j("m =. /\n+ m 1 2 3") == 6
        assert j("c =. &\n2 (+ c *:) 3") == 13


class TestExplain:
    def test_fork_structure_and_shapes(self):
        text = j.compile("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]}).explain()
        assert "+/" in text
        assert "%" in text
        assert "#" in text
        assert "fork" in text
        assert "→ 5 $ float" in text
        assert "→ scalar float" in text

    def test_without_values_the_structure_alone(self):
        text = j.compile("+/ {w} * {x}").explain()
        assert "structure only" in text
        assert "fused kernel" in text
        assert "parameters: w, x" in text

    def test_call_time_data_is_used(self):
        k = j.compile("+/ {w} * {x}", {"w": [1.0, 2.0, 3.0]})
        text = k.explain({"x": [4.0, 5.0, 6.0]})
        assert "structure only" not in text
        assert "[kernel ran]" in text

    def test_named_verb_section(self):
        text = j.compile("mean =. +/ % #\nmean 1 2 3 4").explain()
        assert "verb definition mean" in text
        assert "no runtime work" in text


class TestCli:
    def test_explain_flag(self, capsys):
        from jay._cli import main

        assert main(["--explain", "-e", "(+/ % #) 1 2 3 4"]) == 0
        out = capsys.readouterr().out
        assert "fork" in out
        assert "→ scalar float" in out

    def test_run_without_explain(self, capsys):
        from jay._cli import main

        assert main(["-e", "(+/ % #) 1 2 3 4"]) == 0
        assert capsys.readouterr().out.strip() == "2.5"

    def test_an_expression_reads_the_process_stdin(self, capsys, monkeypatch):
        import io

        from jay._cli import main

        monkeypatch.setattr("sys.stdin", io.StringIO("typed in\n"))
        assert main(["-e", "⍞", "--lang", "apl"]) == 0
        assert capsys.readouterr().out.strip() == "typed in"
