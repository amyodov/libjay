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


class TestDialect:
    def test_index_origin(self):
        assert apl("⍳3").tolist() == [1, 2, 3]
        assert apl.compile("⍳3", index_origin=0)().tolist() == [0, 1, 2]

    def test_create_compiler(self):
        from jay.lang import APL

        c = APL.create_compiler(APL.Dialect(index_base=0))
        assert c("⍳3").tolist() == [0, 1, 2]


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

    def test_apl_function_assignment_is_not_yet(self):
        with pytest.raises(JayError, match="function assignment"):
            apl.compile("F←+/")


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
