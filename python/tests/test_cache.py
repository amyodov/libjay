"""The compiled-program cache: faster, and invisible.

Compiling the same source twice hands back the same program. That is only
allowed to save time, so what is pinned here is that nothing else changes —
kernels stay independent of each other, a refusal stays a refusal, and the
dialect is part of what the table is keyed by.
"""

import numpy as np
import pytest

import jay
from jay import JayError, apl, j


class TestCache:
    def test_the_same_source_compiles_once(self):
        a = j.compile("+/ {x} * {w}")
        b = j.compile("+/ {x} * {w}")
        assert a._inner is b._inner

    def test_different_sources_are_different_programs(self):
        assert j.compile("2+2")._inner is not j.compile("2+3")._inner

    def test_the_languages_do_not_share_a_table(self):
        assert j.compile("2+2")._inner is not apl.compile("2+2")._inner

    def test_the_index_origin_is_part_of_the_key(self):
        zero = apl.compile("⍳3", index_origin=0)
        one = apl.compile("⍳3", index_origin=1)
        assert zero._inner is not one._inner
        assert zero().tolist() == [0, 1, 2]
        assert one().tolist() == [1, 2, 3]

    def test_clearing_gives_a_fresh_program(self):
        first = j.compile("2+2")._inner
        jay.clear_cache()
        assert j.compile("2+2")._inner is not first

    def test_a_refusal_is_raised_every_time(self):
        # An unbalanced parenthesis: "+ + +" no longer serves here, since a
        # bare train is a displayable verb, as it is in J.
        for _ in range(3):
            with pytest.raises(JayError):
                j.compile("+/ (")

    def test_kernels_from_one_source_stay_independent(self):
        x = np.arange(4, dtype=np.float64)
        y = np.arange(4, dtype=np.float64) * 10
        a = j.compile("+/ {v}", {"v": x})
        b = j.compile("+/ {v}", {"v": y})
        assert a._inner is b._inner
        assert a() == 6
        assert b() == 60

    def test_a_deployed_kernel_does_not_disturb_the_cached_one(self):
        plain = j.compile("+/ {x} * {x}")
        if not jay.devices():
            pytest.skip("no GPU adapter on this machine")
        plain.deploy("gpu")
        again = j.compile("+/ {x} * {x}")
        assert again._inner is plain._inner
        assert again.device is None

    def test_the_one_shot_form_still_computes(self):
        for _ in range(3):
            assert j("2 + 2") == 4
        assert j("+/ {v}", {"v": np.arange(5, dtype=np.float64)}) == 10
