"""A Savitzky-Golay filter designed from its least-squares definition
(Savitzky and Golay, Analytical Chemistry 36, 1964): the weights are a row
of the pseudo-inverse of the Vandermonde matrix over the window offsets, and
the filter is the convolution with them."""

from __future__ import annotations

import math

import numpy as np


def coefficients(half: int, degree: int, deriv: int = 0) -> np.ndarray:
    t = np.arange(-half, half + 1, dtype=np.float64)
    a = t[:, None] ** np.arange(degree + 1)
    g = np.linalg.pinv(a)
    return math.factorial(deriv) * g[deriv]


def filt(signal: np.ndarray, coef: np.ndarray) -> np.ndarray:
    """The interior of the filtered signal: one value per full window."""
    win = np.lib.stride_tricks.sliding_window_view(signal, coef.shape[0])
    return win @ coef
