"""An extreme learning machine, from Huang, Zhu and Siew, Neurocomputing 70
(2006): a random hidden layer left untrained and a linear readout fitted by
ridge-regularised least squares."""

from __future__ import annotations

import numpy as np


def hidden(x: np.ndarray, w: np.ndarray, b: np.ndarray) -> np.ndarray:
    return np.tanh(x @ w + b)


def fit(x: np.ndarray, t: np.ndarray, w: np.ndarray, b: np.ndarray, lam: float) -> np.ndarray:
    """The readout, by the normal equations: an h-by-h solve whatever n is."""
    h = hidden(x, w, b)
    a = h.T @ h + lam * np.eye(h.shape[1])
    return np.linalg.solve(a, h.T @ t)


def predict(x: np.ndarray, w: np.ndarray, b: np.ndarray, beta: np.ndarray) -> np.ndarray:
    return hidden(x, w, b) @ beta
