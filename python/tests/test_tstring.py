"""t-string (PEP 750) binding. This file only parses on Python 3.14+."""

import pytest

from libjay import JayError, j


def test_tstring_samples_are_defaults():
    weights = [0.5, 0.5]
    data = [10.0, 30.0]
    k = j.compile(t"+/ {weights} * {data}")
    assert k.params == ["weights", "data"]
    assert k() == 20.0


def test_tstring_rebind_by_name():
    x = [1, 2, 3]
    k = j.compile(t"+/ {x}")
    assert k() == 6
    assert k({"x": [10, 20]}) == 30
    assert k() == 6  # defaults survive call-time overrides


def test_tstring_one_shot():
    x = 21
    assert j(t"2 * {x}") == 42


def test_non_identifier_interpolation_is_rejected():
    d = {"x": 1}
    with pytest.raises((TypeError, JayError), match="identifier"):
        j.compile(t"1 + {d['x']}")


def test_tstring_keeps_arrow_data_alive():
    """The kernel's defaults keep a bound numpy array alive on their own."""
    np = pytest.importorskip("numpy")
    import gc

    prices = np.arange(1000, dtype=np.float64)
    k = j.compile(t"+/ {prices}")
    del prices
    gc.collect()
    assert k() == pytest.approx(499500.0)
