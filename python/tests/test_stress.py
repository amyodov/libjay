"""Usage stress: what repeated, concurrent and failing use does to a process.

Nothing here is a new question about the language. Every case asks the same
programs the same thing thousands of times, or from several threads at once,
or with the pool sized differently, or with refusals interleaved, and holds
every answer to being the answer the first pass gave.

The one measurement that is not an answer is resident memory, compared as a
RATIO against this process's own baseline taken after a warm-up. A megabyte
figure is a property of the machine and its allocator; a ratio is a property
of the code. The programs end in integers, so "the same answer" means exactly
the same answer whatever order a reduction was associated in.
"""

import os
import subprocess
import sys
import threading

import pytest

import jay
from jay import JayError, apl, j

# --- the work --------------------------------------------------------------

# Whole pipelines rather than single primitives: a cycle touches the parser,
# the fusion pass, the allocator and the reductions. Each answers integers.
PROGRAMS = [
    ("j", "n =. 30000\nv =. i. n\ns =. +/ v\nm =. >./ v\n(s , m) , +/ 0 = 7 | v"),
    ("j", "s =. 1000 | i. 5000\ng =. /: s\nsrt =. g { s\n(3 {. srt) , (+/ srt)"),
    ("apl", "N←30000\nV←¯1+⍳N\nS←+/V\nM←⌈/V\n(S,M),+/0=7|V"),
    ("apl", "S←1000|¯1+⍳5000\nG←⍋S\nSR←S[G]\n(3↑SR),(+/SR)"),
]


def _lang(name):
    return j if name == "j" else apl


def digest(value):
    """A program's answer as comparable text: shape and elements."""
    return f"{getattr(value, 'shape', ())}:{value.tolist()}"


def run_all():
    """Compile and run every program once, and be refused twice on the way."""
    out = []
    for name, src in PROGRAMS:
        out.append(digest(_lang(name)(src)))
    with pytest.raises(JayError):
        j.compile("1 + ")
    with pytest.raises(JayError):
        j("1 2 3 + 4 5")
    return out


def rss_kib():
    """This process's resident set in kibibytes, or None where it cannot be
    asked for. The unit does not matter: only the ratio is used."""
    try:
        out = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(os.getpid())],
            capture_output=True,
            text=True,
            timeout=20,
        )
        if out.returncode == 0:
            return int(out.stdout.strip())
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    try:
        import resource

        return int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    except Exception:  # pragma: no cover - a platform without either
        return None


# --- repetition ------------------------------------------------------------


class TestRepetition:
    def test_repeated_compile_and_run_holds_its_memory(self):
        # Every source is distinct, so the compiled-program cache cannot
        # answer for the compiler; the cache is cleared as well, which is
        # where a program that outlived its entry would show up.
        expected = None
        for i in range(30):
            got = self._cycle(i)
            expected = expected or got
            assert got == expected, "an answer moved during the warm-up"
        before = rss_kib()
        for i in range(30, 330):
            assert self._cycle(i) == expected, "an answer moved under repetition"
            if i % 50 == 0:
                jay.clear_cache()
        after = rss_kib()
        if before is None or after is None:
            pytest.skip("resident memory cannot be read on this platform")
        # A ratio, with a small additive slack so that a small baseline
        # cannot make ordinary allocator noise look like a leak.
        ceiling = before * 1.5 + 32_768
        assert after <= ceiling, (
            f"resident memory grew from {before} to {after} over 300 cycles "
            f"(ceiling {ceiling:.0f})"
        )

    @staticmethod
    def _cycle(i):
        # A comment carrying the round makes each source distinct without
        # changing what it computes.
        out = []
        for name, src in PROGRAMS:
            marker = f"NB. {i}\n" if name == "j" else f"⍝ {i}\n"
            kernel = _lang(name).compile(marker + src)
            out.append(digest(kernel()))
        with pytest.raises(JayError):
            j.compile(f"NB. {i}\n1 + ")
        return out

    def test_binding_the_same_kernel_again_and_again(self):
        np = pytest.importorskip("numpy")
        kernel = j.compile("s =. {v}\n(+/ s) , (# s)")
        for n in range(1, 300):
            data = np.arange(n, dtype=np.int64)
            bound = kernel.bind({"v": data})
            assert bound().tolist() == [int(data.sum()), n]
        # The unbound kernel is untouched by every bind that came off it.
        assert kernel.params == ["v"]
        with pytest.raises(JayError):
            kernel()


# --- threads ---------------------------------------------------------------


class TestThreads:
    def test_one_kernel_answers_the_same_from_many_threads(self):
        kernels = [(_lang(n).compile(s), None) for n, s in PROGRAMS]
        wanted = [digest(k()) for k, _ in kernels]
        failures = []

        def worker():
            try:
                for _ in range(25):
                    for (kernel, _), want in zip(kernels, wanted):
                        assert digest(kernel()) == want
            except BaseException as exc:  # noqa: BLE001 - reported, not swallowed
                failures.append(exc)

        threads = [threading.Thread(target=worker) for _ in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert not failures, failures[0]

    def test_bound_kernels_do_not_leak_into_each_other_across_threads(self):
        np = pytest.importorskip("numpy")
        base = j.compile("+/ {v}")
        data = [np.arange(n, dtype=np.int64) for n in (10, 100, 1000)]
        failures = []

        def worker(which):
            try:
                bound = base.bind({"v": data[which]})
                for _ in range(50):
                    assert bound() == int(data[which].sum())
            except BaseException as exc:  # noqa: BLE001
                failures.append(exc)

        threads = [threading.Thread(target=worker, args=(i % 3,)) for i in range(9)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert not failures, failures[0]


# --- the pool --------------------------------------------------------------

_SWEEP = """
import jay
from jay import apl, j
out = []
out.append(j("n =. 30000\\nv =. i. n\\n(+/ v) , (>./ v) , +/ 0 = 7 | v").tolist())
out.append(j("s =. 1000 | i. 5000\\ng =. /: s\\n(3 {. g { s) , (+/ s)").tolist())
out.append(apl("N←30000\\nV←¯1+⍳N\\n((+/V),⌈/V),+/0=7|V").tolist())
print("DIGEST", out)
"""


class TestPoolSize:
    def test_the_thread_count_does_not_change_the_answer(self):
        # LIBJAY_THREADS is read once per process and frozen, so the sweep is
        # three child interpreters rather than three loops.
        answers = {}
        for n in (1, 2, 4):
            env = dict(os.environ, LIBJAY_THREADS=str(n))
            out = subprocess.run(
                [sys.executable, "-c", _SWEEP],
                env=env,
                capture_output=True,
                text=True,
                timeout=300,
            )
            assert out.returncode == 0, out.stderr
            line = next(
                (l for l in out.stdout.splitlines() if l.startswith("DIGEST ")), None
            )
            assert line is not None, out.stdout
            answers[n] = line
        assert len(set(answers.values())) == 1, answers


# --- refusals --------------------------------------------------------------

REFUSALS = [
    ("j", "1 + "),
    ("j", "+/ ("),
    ("apl", "1 2 3+"),
    ("j", "1 2 3 + 4 5"),
    ("apl", "1 2 3+4 5"),
    ("j", "'abc' + 1"),
    ("apl", "1÷0"),
]


class TestRefusals:
    def test_refusals_do_not_poison_what_comes_after(self):
        kernels = [(_lang(n).compile(s), digest(_lang(n)(s))) for n, s in PROGRAMS]
        for _ in range(150):
            for name, src in REFUSALS:
                with pytest.raises(JayError) as caught:
                    _lang(name)(src)
                assert str(caught.value), f"{src!r} was refused without a message"
            for kernel, want in kernels:
                assert digest(kernel()) == want

    def test_a_kernel_survives_the_wrong_data(self):
        np = pytest.importorskip("numpy")
        kernel = j.compile("s =. {v}\n(+/ s) , (# s)")
        good = np.arange(1000, dtype=np.int64)
        want = [int(good.sum()), 1000]
        for _ in range(100):
            with pytest.raises(JayError):
                kernel()
            with pytest.raises(JayError):
                kernel({"nosuchname": good})
            assert kernel({"v": good}).tolist() == want
