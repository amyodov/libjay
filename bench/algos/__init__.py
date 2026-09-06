"""Reference implementations of the algorithms in `examples/algos`.

Each module here is written from the algorithm's own description — the paper
or the textbook — with numpy, and never from the J. That is the point: the
two implementations agree because both follow the mathematics, not because
one was transcribed from the other.

`jsource` reads the definitions out of the matching J file, so the J the
tests and the benchmarks run is the very source the example ships; there is
no second copy to drift.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np

ALGOS = Path(__file__).resolve().parents[2] / "examples" / "algos"
DEMO = "NB. === demo"


def jsource(name: str) -> str:
    """The definitions of `examples/algos/<name>.ijs`, without its demo."""
    text = (ALGOS / f"{name}.ijs").read_text(encoding="utf-8")
    return text.split(DEMO)[0]


#: The generator every example's demo carries, character for character. It
#: is not part of any algorithm — it is how the tests, the corpus and the
#: examples get the same numbers out of three implementations without
#: carrying a literal series around.
RANDOM_J = """RANDOM =. 4 : 0
  p =. 2147483647
  u =. p | 1103515245 * x + >: i. y
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 48271 * u
  _1 + 2 * u % p
)
"""


def hash_random(seed: int, n: int) -> np.ndarray:
    """The `RANDOM` verb of the J files: a counter driven through a Mersenne
    prime's residues, squared twice, answering n numbers in (-1, 1).

    The squaring is what makes it a generator rather than a lattice. A chain
    of multiplications modulo a prime composes into one multiplication, so
    consecutive counters would come out a fixed distance apart and any series
    built from them would carry an autocorrelation of the generator's own —
    which is exactly what a Hurst exponent or a variance ratio is there to
    measure. Every product stays under 2^63, so int64 arithmetic is exact and
    the answer is bit-for-bit what J computes.
    """
    p = np.int64(2147483647)
    a = np.int64(1103515245)
    c = np.int64(12345)
    u = (a * (np.int64(seed) + np.arange(1, n + 1, dtype=np.int64))) % p
    u = (a * ((u * u) % p) + c) % p
    u = (a * ((u * u) % p) + c) % p
    u = (np.int64(48271) * u) % p
    return -1.0 + 2.0 * (u / p)
