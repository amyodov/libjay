"""A Hopfield network, from Hopfield, PNAS 79 (1982): Hebbian storage as the
sum of the patterns' outer products with a cleared diagonal, and synchronous
recall by the sign of the local field."""

from __future__ import annotations

import numpy as np


def store(patterns: np.ndarray) -> np.ndarray:
    """Weights from a p-by-n matrix of -1/+1 patterns."""
    n = patterns.shape[1]
    w = patterns.T @ patterns
    np.fill_diagonal(w, 0.0)
    return w / n


def flip(states: np.ndarray, w: np.ndarray) -> np.ndarray:
    """One synchronous sweep; a unit with no field keeps its state."""
    f = states @ w
    return np.where(f == 0.0, states, np.sign(f))


def recall(probes: np.ndarray, w: np.ndarray, sweeps: int) -> np.ndarray:
    s = probes
    for _ in range(sweeps):
        s = flip(s, w)
    return s


def energy(states: np.ndarray, w: np.ndarray) -> np.ndarray:
    return -0.5 * ((states @ w) * states).sum(axis=-1)
