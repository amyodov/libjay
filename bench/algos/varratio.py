"""The Lo-MacKinlay variance-ratio test, from the equations of Lo and
MacKinlay, *Stock Market Prices Do Not Follow Random Walks*, Review of
Financial Studies 1 (1988): the overlapping estimator with the unbiased
corrections, and both the homoskedastic and the robust statistic."""

from __future__ import annotations

import numpy as np


def variance_ratio(logprice: np.ndarray, q: int) -> tuple[float, float, float]:
    """Answers (ratio, z1, z2) for a horizon of q bars."""
    n = logprice.shape[0] - 1
    mu = (logprice[-1] - logprice[0]) / n
    e = np.diff(logprice) - mu
    s2 = e * e
    var_a = s2.sum() / (n - 1)

    ec = (logprice[q:] - logprice[:-q]) - q * mu
    m = q * (n - q + 1) * (1.0 - q / n)
    var_c = (ec * ec).sum() / m

    vr = var_c / var_a
    phi = 2.0 * (2 * q - 1) * (q - 1) / (3.0 * q)
    z1 = np.sqrt(n) * (vr - 1.0) / np.sqrt(phi)

    denom = s2.sum() ** 2
    theta = 0.0
    for j in range(1, q):
        delta = n * float((s2[j:] * s2[:-j]).sum()) / denom
        theta += (2.0 * (q - j) / q) ** 2 * delta
    z2 = np.sqrt(n) * (vr - 1.0) / np.sqrt(theta)
    return float(vr), float(z1), float(z2)


def variance_ratios(logprice: np.ndarray, qs) -> np.ndarray:
    return np.array([variance_ratio(logprice, int(q)) for q in qs])
