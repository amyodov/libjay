"""The Hurst exponent by rescaled range, from Hurst's definition as
Mandelbrot and Wallis state it: within each non-overlapping block, the range
of the cumulative deviation from the block mean over the block's standard
deviation, averaged across blocks and regressed on the block length in
logs."""

from __future__ import annotations

import numpy as np


def rescaled_range(series: np.ndarray, length: int) -> float:
    """The mean R/S over every whole block of `length` items."""
    k = series.shape[0] // length
    block = series[: k * length].reshape(k, length)
    dev = block - block.mean(axis=1, keepdims=True)
    cs = np.cumsum(dev, axis=1)
    r = cs.max(axis=1) - cs.min(axis=1)
    s = np.sqrt((dev * dev).sum(axis=1) / length)
    return float((r / s).sum() / k)


def hurst(series: np.ndarray, lengths) -> tuple[float, float]:
    """Answers (exponent, intercept) of the least-squares log-log fit."""
    lx = np.log(np.asarray(lengths, dtype=np.float64))
    ly = np.array([rescaled_range(series, int(m)) for m in lengths])
    ly = np.log(ly)
    dx = lx - lx.mean()
    h = float((dx * (ly - ly.mean())).sum() / (dx * dx).sum())
    return h, float(ly.mean() - h * lx.mean())
