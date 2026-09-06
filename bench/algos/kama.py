"""Kaufman's Adaptive Moving Average, from the description in Kaufman's
*New Trading Systems and Methods*: an efficiency ratio between a fast and a
slow exponential constant, squared, driving the usual exponential update."""

from __future__ import annotations

import numpy as np


def efficiency_ratio(price: np.ndarray, period: int) -> np.ndarray:
    """Net displacement over `period` bars divided by the path walked."""
    move = np.abs(np.diff(price))
    path = np.convolve(move, np.ones(period), mode="valid")
    net = np.abs(price[period:] - price[:-period])
    out = np.zeros(net.shape, dtype=np.float64)
    np.divide(net, path, out=out, where=path != 0.0)
    return out


def smoothing(price: np.ndarray, period: int, fast: int, slow: int) -> np.ndarray:
    f = 2.0 / (fast + 1)
    s = 2.0 / (slow + 1)
    return (efficiency_ratio(price, period) * (f - s) + s) ** 2


def kama(price: np.ndarray, period: int = 10, fast: int = 2, slow: int = 30) -> np.ndarray:
    sc = smoothing(price, period, fast, slow)
    q = price[period:]
    out = np.empty(q.shape, dtype=np.float64)
    out[0] = q[0]
    prev = out[0]
    for i in range(1, q.shape[0]):
        prev = prev + sc[i] * (q[i] - prev)
        out[i] = prev
    return out


try:
    from numba import njit
except ImportError:  # pragma: no cover - numba is optional
    kama_numba = None
else:

    @njit(cache=True)
    def _recur(sc, q):
        out = np.empty(q.shape[0])
        prev = q[0]
        out[0] = prev
        for i in range(1, q.shape[0]):
            prev = prev + sc[i] * (q[i] - prev)
            out[i] = prev
        return out

    def kama_numba(price, period=10, fast=2, slow=30):
        return _recur(smoothing(price, period, fast, slow), price[period:])
