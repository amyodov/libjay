"""One check per algorithm: a J program, the numpy answer it must match,
and the tolerance that separates the two implementations' rounding from a
disagreement.

The J is the example's own definitions plus a few hundred bars of data built
by the shared generator, so the very same string can be handed to libjay, to
jconsole, or to anything else that reads J — which is what makes the check
differential rather than self-referential. The numpy side is written from
the algorithm's mathematics and never from the J.

Every program's last sentence is a COLUMN (`,.`), one number per line, so an
interpreter that prints its answer can be read back without guessing where
one number ends and the next begins.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable

import numpy as np

from . import RANDOM_J, hash_random, jsource
from . import elm as r_elm
from . import hopfield as r_hop
from . import hurst as r_hurst
from . import kama as r_kama
from . import savgol as r_sg
from . import varratio as r_vr


@dataclass(frozen=True)
class Case:
    name: str
    tail: str
    ref: Callable[[], np.ndarray]
    tol: float
    why: str

    @property
    def program(self) -> str:
        return jsource(self.name) + RANDOM_J + self.tail


# ------------------------------------------------------------- the references


def _kama() -> np.ndarray:
    close = 100.0 * np.exp(np.cumsum(0.002 * hash_random(7, 500)))
    return r_kama.kama(close, 10, 2, 30)


def _varratio() -> np.ndarray:
    w = hash_random(7, 3000)
    walk = np.cumsum(0.01 * (w + 0.6 * np.roll(w, 1)))
    return r_vr.variance_ratios(walk, [2, 3, 4, 5, 8]).ravel()


LENGTHS = [16, 32, 64, 128, 256, 512]


def _hurst() -> np.ndarray:
    w = hash_random(11, 8192)
    return np.concatenate([
        np.array(r_hurst.hurst(w, LENGTHS)),
        np.array([r_hurst.rescaled_range(w, m) for m in LENGTHS]),
    ])


def _elm_data():
    n, d, h = 400, 3, 40
    x = hash_random(2, n * d).reshape(n, d)
    t = np.sin(3.0 * x[:, 0:1]) + 0.5 * (x[:, 1:2] * x[:, 2:3])
    w = 4.0 * hash_random(5, d * h).reshape(d, h)
    b = 2.0 * hash_random(9, h)
    return x, t, w, b


def _elm() -> np.ndarray:
    x, t, w, b = _elm_data()
    beta = r_elm.fit(x, t, w, b, 0.1)
    return r_elm.predict(x, w, b, beta).ravel()


def _hopfield() -> np.ndarray:
    n, p, q = 64, 3, 5
    pat = np.sign(hash_random(17, p * n)).reshape(p, n)
    w = r_hop.store(pat)
    probe = np.sign(hash_random(29, q * n)).reshape(q, n)
    rec = r_hop.recall(probe, w, 6)
    return np.concatenate([rec.ravel(), r_hop.energy(rec, w), r_hop.energy(probe, w)])


def _savgol() -> np.ndarray:
    n = 400
    t = np.arange(n) / n
    sig = np.sin(6 * t) + 0.1 * hash_random(13, n)
    return np.concatenate([
        r_sg.coefficients(2, 2, 0),
        r_sg.coefficients(4, 3, 0),
        r_sg.coefficients(4, 3, 1),
        r_sg.coefficients(6, 4, 2),
        r_sg.filt(sig, r_sg.coefficients(4, 3, 0))[:30],
        r_sg.filt(sig, r_sg.coefficients(4, 3, 1))[:30],
    ])


CASES = [
    Case(
        "kama", """
n =. 500
close =. 100 * ^ +/\\ 0.002 * 7 RANDOM n
,. 10 2 30 KAMA close
""", _kama, 1e-12,
        "a 490-step recursion; the error the two orderings can accumulate is "
        "bounded by the smoothing constant, which is well under one",
    ),
    Case(
        "varratio", """
n =. 3000
w =. 7 RANDOM n
walk =. +/\\ 0.01 * w + 0.6 * _1 |. w
,. , (2 3 4 5 8) VR"0 _ walk
""", _varratio, 1e-11,
        "sums of 3000 squares, and a ratio of two of them; the running sum "
        "orders differ but nothing cancels",
    ),
    Case(
        "hurst", """
n =. 8192
w =. 11 RANDOM n
lens =. 16 32 64 128 256 512
,. (lens HURST w) , lens RS"0 _ w
""", _hurst, 1e-11,
        "a range of running sums, then a least-squares slope over six points",
    ),
    Case(
        "elm", """
n =. 400
d =. 3
h =. 40
x =. (n, d) $ 2 RANDOM n * d
t =. (,. 1 o. 3 * 0 {"1 x) + 0.5 * ,. (1 {"1 x) * 2 {"1 x
w =. (d, h) $ 4 * 5 RANDOM d * h
b =. 2 * 9 RANDOM h
beta =. (w ; b ; 0.1) ELMFIT x ; t
,. , (w ; b ; beta) PREDICT x
""", _elm, 1e-8,
        "a 40-by-40 ridge solve whose condition number is about 5e4: libjay "
        "and numpy factor it their own ways, so the readout is only as "
        "reproducible as the conditioning allows. What it PREDICTS is the "
        "stable part, and that is what is compared",
    ),
    Case(
        "hopfield", """
n =. 64
p =. 3
q =. 5
pat =. * (p, n) $ 17 RANDOM p * n
w =. STORE pat
probe =. * (q, n) $ 29 RANDOM q * n
rec =. (w ; 6) RECALL probe
,. (, rec) , (w ENERGY rec) , w ENERGY probe
""", _hopfield, 1e-12,
        "the states are exactly _1 and 1 and must match cell for cell; only "
        "the energies are floating point",
    ),
    Case(
        "savgol", """
n =. 400
t =. (i. n) % n
sig =. (1 o. 6 * t) + 0.1 * 13 RANDOM n
c =. SGCOEF 4 3 0
c1 =. SGCOEF 4 3 1
,. (SGCOEF 2 2 0) , c , c1 , (SGCOEF 6 4 2) , (30 {. c SGFILT sig) , 30 {. c1 SGFILT sig
""", _savgol, 1e-10,
        "the widest design here is a 13-by-5 Vandermonde over _6 to 6, whose "
        "normal equations libjay inverts where numpy takes a pseudo-inverse "
        "of the matrix itself — two different routes to the same weights, "
        "which agree to about 2e-13",
    ),
]

BY_NAME = {c.name: c for c in CASES}
