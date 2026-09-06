"""The program-scale corpus entries for the six algorithms.

Each entry is one whole program: the J definitions of `examples/algos/*.ijs`
verbatim (comments and blank lines stripped, so the line stays readable as a
diff), the shared deterministic generator, a few hundred bars of data built
inside J, and a last sentence that rounds the answer into integers.

    python -m bench.algos.corpus        # rewrite the corpus file

The rounding is what makes the recording exact rather than merely close: the
snapshot holds jconsole's integers, and a change of answer names itself in
the diff. Regenerate whenever a definition in `examples/algos` changes, then
`cargo run -p libjay-devtools -- record j algorithms`.
"""

from __future__ import annotations

import re
from pathlib import Path

from . import RANDOM_J, jsource

CORPUS = Path(__file__).resolve().parents[2] / "crates/libjay/tests/corpus/j/algorithms.txt"

HEADER = """// Six number-crunching algorithms, each written in J from its own
// mathematics: three from technical analysis, one neural network, one
// filter design, one clustering-free learner. docs/algorithms.md is the
// prose; examples/algos/*.ijs is where the definitions live, and these
// lines are those definitions with a few hundred bars of data behind them.
//
// The data is built inside J by a counter-based generator whose every
// product stays under 2^63, so the input is exactly the same number in
// every implementation and no literal series has to be carried here. The
// last sentence rounds into integers, so what is recorded is exact.
//
// Regenerate with `python -m bench.algos.corpus` after editing a
// definition; the source is never edited here.
"""

PROGRAMS = [
    (
        "kama",
        "// Kaufman's adaptive moving average over 200 bars of a random walk:\n"
        "// its length, the first three and last three values and the mean, in\n"
        "// hundredths of a price unit.",
        """n =. 200
close =. 100 * ^ +/\\ 0.002 * 7 RANDOM n
k =. 10 2 30 KAMA close
(# k) , <. 0.5 + 100 * (3 {. k) , (_3 {. k) , (+/ k) % # k""",
    ),
    (
        "varratio",
        "// The Lo-MacKinlay variance ratio at three horizons, over a walk whose\n"
        "// increments carry six tenths of the previous increment — so the ratio\n"
        "// must come out above 1 and the statistics must reject the walk. Ratio\n"
        "// and both z scores, in thousandths.",
        """n =. 1200
w =. 7 RANDOM n
walk =. +/\\ 0.01 * w + 0.6 * _1 |. w
<. 0.5 + 1000 * , (2 4 8) VR"0 _ walk""",
    ),
    (
        "hurst",
        "// The Hurst exponent of white noise by rescaled range over five block\n"
        "// lengths: the exponent and intercept, then the five R/S values, in\n"
        "// ten-thousandths. White noise sits near a half.",
        """n =. 2048
w =. 11 RANDOM n
lens =. 16 32 64 128 256
<. 0.5 + 10000 * (lens HURST w) , lens RS"0 _ w""",
    ),
    (
        "elm",
        "// An extreme learning machine with 20 hidden units fitted to 200 points\n"
        "// of a sine plus an interaction term, and asked for its first eight\n"
        "// predictions and its training error, in thousandths. The readout\n"
        "// itself is not reported: a ridge solve is only as reproducible as its\n"
        "// conditioning, while what it predicts is stable.",
        """n =. 200
d =. 3
h =. 20
x =. (n, d) $ 2 RANDOM n * d
t =. (,. 1 o. 3 * 0 {"1 x) + 0.5 * ,. (1 {"1 x) * 2 {"1 x
w =. (d, h) $ 4 * 5 RANDOM d * h
b =. 2 * 9 RANDOM h
net =. w ; b ; 0.1
beta =. net ELMFIT x ; t
p =. (w ; b ; beta) PREDICT x
<. 0.5 + 1000 * (, 8 {. p) , %: (+/ *: (, p) - , t) % n""",
    ),
    (
        "hopfield",
        "// Three patterns over 64 units stored by the Hebbian rule, then each\n"
        "// pattern corrupted in twelve places and recalled for six sweeps: the\n"
        "// bits wrong before and after, and the energies in hundredths. Recall\n"
        "// repairs the probe and drives the energy down, never up.",
        """n =. 64
p =. 3
pat =. * (p, n) $ 17 RANDOM p * n
w =. STORE pat
mask =. _1 ^ 12 > /:"1 /:"1 (p, n) $ 23 RANDOM p * n
probe =. pat * mask
rec =. (w ; 6) RECALL probe
(+/"1 probe ~: pat) , (+/"1 rec ~: pat) , <. 0.5 + 100 * (w ENERGY probe) , w ENERGY rec""",
    ),
    (
        "savgol",
        "// Savitzky-Golay weights designed three ways — the classic five-point\n"
        "// quadratic times 35, which is the published _3 12 17 12 _3, a nine\n"
        "// point cubic times 231, and a first derivative times 60 — then the\n"
        "// smoother run over 400 samples of a noisy sine, reported as the\n"
        "// first four values and the noise left, in ten-thousandths.",
        """n =. 400
t =. (i. n) % n
sig =. (1 o. 6 * t) + 0.1 * 13 RANDOM n
c =. SGCOEF 4 3 0
s =. c SGFILT sig
(<. 0.5 + 35 * SGCOEF 2 2 0) , (<. 0.5 + 231 * c) , (<. 0.5 + 60 * SGCOEF 4 3 1) , <. 0.5 + 10000 * (4 {. s) , %: (+/ *: s - 4 }. _4 }. 1 o. 6 * t) % # s""",
    ),
]


def strip_comments(text: str) -> str:
    keep = []
    for line in text.splitlines():
        if not line.strip() or re.match(r"^\s*NB\.", line):
            continue
        keep.append(line.rstrip())
    return "\n".join(keep)


def one_line(name: str, tail: str) -> str:
    body = strip_comments(jsource(name) + RANDOM_J + tail)
    return body.replace("\\", "\\\\").replace("\n", "\\n")


def render() -> str:
    out = [HEADER]
    for name, comment, tail in PROGRAMS:
        out.append(f"\n// --- {name} " + "-" * (66 - len(name)) + "\n")
        out.append(comment + "\n")
        out.append(one_line(name, tail) + "\n")
    return "".join(out)


if __name__ == "__main__":
    CORPUS.write_text(render(), encoding="utf-8")
    print(f"wrote {CORPUS} ({len(PROGRAMS)} programs)")
