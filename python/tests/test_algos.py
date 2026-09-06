"""The algorithms of `examples/algos` against their numpy references.

Each case runs the example's own J definitions — read from the file, not
copied here — over data both sides generate identically, and compares the
answer with an implementation written from the algorithm's mathematics in
numpy. What is asserted is the numbers, on data; nothing here inspects how
libjay reached them.

The references live in `bench/algos`, which is not an installed package, so
the module is reached by path. numpy is what the references are written in;
without it the whole file skips.
"""

import sys
from pathlib import Path

import pytest

import jay

np = pytest.importorskip("numpy")

BENCH = Path(__file__).resolve().parents[2] / "bench"
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

from algos import RANDOM_J, jsource  # noqa: E402
from algos.cases import CASES  # noqa: E402
from algos.corpus import CORPUS, PROGRAMS, render  # noqa: E402


def worst_relative(got, want):
    """The largest relative difference, floored so that an element which
    happens to sit near zero — a smoothed signal crossing its own mean, a
    prediction at a root — is judged against the answer's own scale rather
    than against its own vanishing magnitude, which no implementation can
    agree on to a relative digit."""
    got = np.asarray(got, dtype=np.float64).ravel()
    want = np.asarray(want, dtype=np.float64).ravel()
    assert got.shape == want.shape, f"{got.shape} against {want.shape}"
    floor = 1e-3 * max(float(np.max(np.abs(want))), 1e-300)
    scale = np.maximum(np.maximum(np.abs(got), np.abs(want)), floor)
    return float(np.max(np.abs(got - want) / scale))


def answer(program):
    """What libjay makes of the program, as a flat vector of doubles."""
    return np.asarray(jay.j(program).tolist(), dtype=np.float64).ravel()


@pytest.mark.parametrize("case", CASES, ids=lambda c: c.name)
def test_the_j_matches_the_numpy_reference(case):
    assert worst_relative(answer(case.program), case.ref()) <= case.tol, case.why


@pytest.mark.parametrize("case", CASES, ids=lambda c: c.name)
def test_the_answer_is_finite(case):
    assert np.all(np.isfinite(answer(case.program)))


@pytest.mark.parametrize("name", [c.name for c in CASES])
def test_the_example_runs_whole(name, capsys):
    """The file as it ships — definitions, demo and all — evaluates."""
    source = (BENCH.parent / "examples" / "algos" / f"{name}.ijs").read_text(encoding="utf-8")
    jay.j(source)
    assert capsys.readouterr().out


@pytest.mark.parametrize("name", [c.name for c in CASES])
def test_the_example_carries_the_shared_generator(name):
    """`bench/algos` and the examples must agree on the generator, or the
    data the reference computes over is not the data J computed over."""
    source = (BENCH.parent / "examples" / "algos" / f"{name}.ijs").read_text(encoding="utf-8")
    if "RANDOM" in source:
        assert RANDOM_J.strip() in source


def test_the_recorded_corpus_is_the_examples_as_they_are_now():
    """The corpus lines are a pinned copy of the same definitions; if an
    example changes without `python -m bench.algos.corpus`, jconsole's
    recorded answers are for source that no longer exists."""
    assert CORPUS.read_text(encoding="utf-8") == render()


@pytest.mark.parametrize("name", [name for name, _, _ in PROGRAMS])
def test_every_algorithm_has_definitions_to_read(name):
    assert "=." in jsource(name)


# --------------------------------------------------- the corpora, not toys

from data import make_audio, make_close  # noqa: E402

from algos import hurst as r_hurst  # noqa: E402
from algos import kama as r_kama  # noqa: E402
from algos import savgol as r_sg  # noqa: E402
from algos import varratio as r_vr  # noqa: E402

BARS = 20_000
BLOCKS = [16, 32, 64, 128, 256, 512]


def _close():
    return make_close(BARS)


def _logprice():
    return np.log(make_close(BARS))


def _returns():
    return np.diff(np.log(make_close(BARS)))


CORPUS_CASES = [
    ("kama", "10 2 30 KAMA {x}", _close,
     lambda x: r_kama.kama(x, 10, 2, 30), 1e-12),
    ("varratio", '8 VR {x}', _logprice,
     lambda x: np.array(r_vr.variance_ratio(x, 8)), 1e-11),
    ("hurst", "16 32 64 128 256 512 HURST {x}", _returns,
     lambda x: np.array(r_hurst.hurst(x, BLOCKS)), 1e-11),
    ("savgol", "(SGCOEF 8 3 0) SGFILT {x}", lambda: make_audio(BARS),
     lambda x: r_sg.filt(x, r_sg.coefficients(8, 3, 0)), 1e-10),
]


@pytest.mark.parametrize("name,tail,build,ref,tol", CORPUS_CASES, ids=[c[0] for c in CORPUS_CASES])
def test_on_the_benchmark_corpora(name, tail, build, ref, tol):
    """The same definitions over the OHLCV close and the DSP signal the
    benchmarks use — a real series of twenty thousand bars, handed to libjay
    as numpy memory rather than built inside J."""
    x = build()
    got = np.asarray(jay.j(jsource(name) + tail, {"x": x}).tolist(), dtype=np.float64).ravel()
    assert worst_relative(got, ref(x)) <= tol
