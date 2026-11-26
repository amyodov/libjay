"""The native code generators, as an installation reports and uses them.

``LIBJAY_JIT`` is read once per process, so each policy runs in a child
interpreter; every generator built in must answer what the interpreter
answers, to the bit.
"""

import os
import subprocess
import sys

import pytest

import jay

# A fold whose step lowers to native code: forced, any length compiles.
FOLD = "0 (] F.. (4 : 'y + 0.5 * x - y')) 0.001 * i. 5000"
CELLS = "(3 : 'if. y > 2.5 do. y * y else. y - 1 end.')\"0 ] 0.001 * i. 5000"


def answer(policy: str, sentence: str) -> tuple[str, str]:
    """The answer's exact value as text, and whatever went to standard error."""
    code = f"import jay; v = jay.j({sentence!r}); print(repr(v.tolist() if hasattr(v, 'tolist') else v))"
    env = dict(os.environ, LIBJAY_JIT=policy)
    run = subprocess.run(
        [sys.executable, "-c", code], env=env, capture_output=True, text=True, check=True
    )
    return run.stdout, run.stderr


class TestBackends:
    def test_names_are_known_and_ordered(self):
        names = jay.jit_backends()
        assert isinstance(names, tuple)
        assert set(names) <= {"cranelift", "llvm"}
        assert list(names) == [n for n in ("cranelift", "llvm") if n in names]

    def test_cranelift_is_in_every_build(self):
        assert "cranelift" in jay.jit_backends()

    @pytest.mark.parametrize("sentence", [FOLD, CELLS], ids=["fold", "cells"])
    @pytest.mark.parametrize("backend", ["cranelift", "llvm"])
    def test_each_generator_answers_as_the_interpreter(self, backend, sentence):
        if backend not in jay.jit_backends():
            pytest.skip(f"{backend} is not built into this installation")
        interpreted, _ = answer("0", sentence)
        compiled, err = answer(backend, sentence)
        assert compiled == interpreted
        assert "libjay" not in err
