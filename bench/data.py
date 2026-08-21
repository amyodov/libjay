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


BARS_PER_DAY = 1440


def make_ohlcv(rows: int) -> dict[str, np.ndarray]:
    """Minute bars: open, high, low, close, volume and a day index.

    The close is `make_close`'s reflected walk, so every price series in the
    benchmarks is the same one. Each bar opens at the previous bar's close,
    its high is at or above both ends and its low at or below both, and the
    volume is lognormal — the relationships an indicator assumes. `day` is
    the bar's index divided by a 1440-minute trading day, so it is already
    sorted, which is what a group-by over minute bars looks like in practice.

    Everything is C-contiguous f64 but `day`, which is i64.
    """
    close = make_close(rows)
    rng = np.random.default_rng(SEED + 3)

    open_ = np.empty_like(close)
    open_[0] = close[0]
    open_[1:] = close[:-1]

    hi = np.maximum(open_, close)
    hi *= 1.0 + np.abs(rng.standard_normal(rows)) * 0.0015
    lo = np.minimum(open_, close)
    lo *= 1.0 - np.abs(rng.standard_normal(rows)) * 0.0015

    volume = rng.standard_normal(rows)
    volume *= 0.4
    volume += 9.0
    np.exp(volume, out=volume)

    day = np.arange(rows, dtype=np.int64) // BARS_PER_DAY
    return {"open": open_, "high": hi, "low": lo, "close": close,
            "volume": volume, "day": day}


AUDIO_RATE = 48000.0


def make_audio(samples: int) -> np.ndarray:
    """A synthetic mono signal: three sines and white noise, C-contiguous f64.

    440 Hz with two partials above it and noise at -20 dB, sampled at 48 kHz
    — enough structure for a low-pass to change, for peak detection to find
    something periodic, and for a single-bin detector to have a bin worth
    detecting.
    """
    rng = np.random.default_rng(SEED + 4)
    t = np.arange(samples) / AUDIO_RATE
    x = np.sin((2.0 * np.pi * 440.0) * t)
    x += 0.5 * np.sin((2.0 * np.pi * 997.0) * t)
    x += 0.25 * np.sin((2.0 * np.pi * 3000.0) * t)
    x += 0.1 * rng.standard_normal(samples)
    return np.ascontiguousarray(x)


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
