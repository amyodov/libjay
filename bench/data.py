"""Benchmark inputs and the timing protocol, shared by the harness and the
worker so both processes measure the very same numbers."""

from __future__ import annotations

import time

import numpy as np

SEED = 20260820


def make_vectors(rows: int) -> tuple[np.ndarray, np.ndarray]:
    """Weights and values: two C-contiguous f64 vectors."""
    rng = np.random.default_rng(SEED)
    w = rng.random(rows)
    x = rng.standard_normal(rows)
    return w, x


CLOSE_STEP = 0.002
CLOSE_BAND = 0.35


def make_close(rows: int) -> np.ndarray:
    """A synthetic OHLCV close series: a random walk in log price, reflected
    at a band, C-contiguous f64.

    An unreflected walk drifts a long way over 20M steps — several orders of
    magnitude in price — which says nothing about a rolling window but does
    make the series unrealistic. Reflecting the walk in a band keeps the
    price near 100 while leaving every step's size untouched, and with it the
    ratio of the rolling standard deviation to the rolling mean, which is
    what decides how much cancellation the variance identity
    (E[x^2] - E[x]^2) has to survive.
    """
    rng = np.random.default_rng(SEED + 2)
    steps = rng.standard_normal(rows) * CLOSE_STEP
    b = CLOSE_BAND
    # A triangle wave of period 4b and amplitude b: slope +-1 everywhere but
    # at the turning points, so each step keeps its own size.
    logp = b - np.abs(np.mod(np.cumsum(steps) + b, 4.0 * b) - 2.0 * b)
    return np.ascontiguousarray(100.0 * np.exp(logp))


def make_matrix(rows: int, cols: int) -> np.ndarray:
    """An OHLCV-shaped block: rows leading, C-contiguous f64."""
    rng = np.random.default_rng(SEED + 1)
    return np.ascontiguousarray(rng.random((rows, cols)))


def best_of(f, repeat: int) -> tuple[float, object]:
    """One warmup call, then the best wall time of `repeat` calls.

    The best rather than the mean: it is the measurement least polluted by
    whatever else the machine was doing.
    """
    value = f()
    best = float("inf")
    for _ in range(repeat):
        t0 = time.perf_counter()
        value = f()
        best = min(best, time.perf_counter() - t0)
    return best, value


def checksum(value) -> float:
    """A single float standing for a result, so the harness can check that
    every implementation computed the same thing."""
    if hasattr(value, "tolist"):
        value = value.tolist()
    if isinstance(value, (list, tuple)):
        return float(sum(float(v) for v in value))
    return float(value)
