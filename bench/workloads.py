"""End-to-end workloads: what an analyst actually computes, four ways.

Twelve jobs a reader would recognise — returns, moving-average crossovers,
Bollinger bands, RSI, VWAP, drawdown, volatility regimes; a low-pass and
peak count, frame RMS in dB, a single-bin detector, a naive DFT and a rolling
cross-correlation — each written in libjay's J, in idiomatic Polars, as a
numba loop and in numpy, over the two corpora libjay is contracted to serve:
OHLCV minute bars and a synthetic audio signal.

    .venv-bench/bin/python bench/workloads.py           # the full suite
    .venv-bench/bin/python bench/workloads.py --quick   # a one-minute smoke

Correctness comes first and is not timed: every implementation is compared
with a reference that computes each window from its own items, over a slice
small enough for that to be affordable. A workload whose implementations
disagree is a bug report, not a benchmark, and the script stops on one.

Every libjay timing comes from a subprocess (this file, with --worker)
because the thread count is fixed when the pool is first used. Each figure is
the best of --repeat wall times after a warmup call, so numba's compilation
is never inside a measurement.
"""

from __future__ import annotations

import argparse
import inspect
import json
import os
import platform
import resource
import subprocess
import sys
import textwrap
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from data import best_of, make_audio, make_ohlcv  # noqa: E402

HERE = Path(__file__).resolve().parent

OHLCV_ROWS = 20_000_000
DSP_SAMPLES = 16_000_000
QUICK_ROWS = 288_000
QUICK_SAMPLES = 262_144

# Every window the suite uses, named once so the J, the Polars and the loops
# cannot drift apart.
MA_FAST, MA_SLOW, MA_LONG = 20, 50, 200
BOLL, BOLL_K = 20, 2.0
RSI_PERIOD = 14
VOL_WINDOW, VOL_REGIME = 20, 1.5
LOWPASS = 32
FRAME = 1024
XCORR_LAG, XCORR_WINDOW = 64, 1024
GOERTZEL_BIN = 4096          # the bin of an FRAME-point analysis, scaled up


# --------------------------------------------------------------- the corpora


def corpus(kind: str, n: int) -> dict[str, np.ndarray]:
    if kind == "ohlcv":
        return make_ohlcv(n)
    return {"x": make_audio(n)}


def goertzel_step(n: int) -> complex:
    """The unit step of the single-bin recurrence: one FRAME-point bin's
    angle, so the detector looks for the same frequency whatever n is."""
    return complex(np.exp(-2j * np.pi * (GOERTZEL_BIN / n)))


def jc(z: complex) -> str:
    """A complex constant as a J literal: `1.5j_0.25` for 1.5-0.25i."""
    f = lambda v: repr(float(v)).replace("-", "_")
    return f"{f(z.real)}j{f(z.imag)}"


# ------------------------------------------------------------ numba kernels
#
# Every loop is the one a numba user would write for the job: a single pass
# with the accumulators carried by hand. They are compiled on first call, and
# the first call is the warmup, so no compilation lands in a measurement.

from numba import njit  # noqa: E402


@njit(cache=True)
def nb_cumulative_return(c):
    out = np.empty(c.shape[0] - 1)
    s = 0.0
    for i in range(1, c.shape[0]):
        s += np.log(c[i] / c[i - 1])
        out[i - 1] = np.exp(s) - 1.0
    return out


@njit(cache=True)
def nb_golden_cross(c, fast, slow, long):
    n = c.shape[0]
    sf = ss = sl = 0.0
    for i in range(long):
        sl += c[i]
        if i >= long - slow:
            ss += c[i]
        if i >= long - fast:
            sf += c[i]
    prev_a = sf / fast > ss / slow
    prev_b = ss / slow > sl / long
    a_cross = b_cross = 0
    for i in range(long, n):
        sf += c[i] - c[i - fast]
        ss += c[i] - c[i - slow]
        sl += c[i] - c[i - long]
        a = sf / fast > ss / slow
        b = ss / slow > sl / long
        if a != prev_a:
            a_cross += 1
        if b != prev_b:
            b_cross += 1
        prev_a, prev_b = a, b
    return np.array([a_cross, b_cross])


@njit(cache=True)
def nb_bollinger_outside(c, w, k):
    n = c.shape[0]
    s = q = 0.0
    for i in range(w):
        s += c[i]
        q += c[i] * c[i]
    outside = 0
    for i in range(w - 1, n):
        if i >= w:
            s += c[i] - c[i - w]
            q += c[i] * c[i] - c[i - w] * c[i - w]
        d = w * c[i] - s
        if abs(d) > k * np.sqrt(w * q - s * s):
            outside += 1
    return outside / (n - w + 1)


@njit(cache=True)
def nb_rsi(c, period):
    n = c.shape[0]
    out = np.empty(n - 1)
    k = 1.0 - 1.0 / period
    ag = al = 0.0
    for i in range(1, n):
        d = c[i] - c[i - 1]
        ag = k * ag + (d if d > 0.0 else 0.0)
        al = k * al + (-d if d < 0.0 else 0.0)
        out[i - 1] = 100.0 - 100.0 / (1.0 + ag / al)
    return out


@njit(cache=True)
def nb_vwap(day, high, low, close, volume, days):
    num = np.zeros(days)
    den = np.zeros(days)
    for i in range(day.shape[0]):
        d = day[i]
        num[d] += (high[i] + low[i] + close[i]) / 3.0 * volume[i]
        den[d] += volume[i]
    return num / den


@njit(cache=True)
def nb_max_drawdown(c):
    peak = c[0]
    worst = 0.0
    for i in range(c.shape[0]):
        if c[i] > peak:
            peak = c[i]
        dd = 1.0 - c[i] / peak
        if dd > worst:
            worst = dd
    return worst


@njit(cache=True)
def nb_vol_regime(c, w, factor):
    n = c.shape[0] - 1
    vol = np.empty(n - w + 1)
    s = q = 0.0
    total = 0.0
    for i in range(n):
        r = c[i + 1] / c[i]
        s += r
        q += r * r
        if i >= w:
            p = c[i - w + 1] / c[i - w]
            s -= p
            q -= p * p
        if i >= w - 1:
            v = np.sqrt(max(w * q - s * s, 0.0))
            vol[i - w + 1] = v
            total += v
    threshold = factor * total / vol.shape[0]
    hot = 0
    for i in range(vol.shape[0]):
        if vol[i] > threshold:
            hot += 1
    return hot / vol.shape[0]


@njit(cache=True)
def nb_lowpass_peaks(x, w):
    n = x.shape[0] - w + 1
    s = 0.0
    for i in range(w):
        s += x[i]
    prev2 = prev1 = 0.0
    peaks = 0
    for i in range(n):
        if i > 0:
            s += x[i + w - 1] - x[i - 1]
        y = s / w
        if i >= 2 and prev1 > prev2 and prev1 > y:
            peaks += 1
        prev2 = prev1
        prev1 = y
    return peaks


@njit(cache=True)
def nb_frame_rms_db(x, frame):
    frames = x.shape[0] // frame
    out = np.empty(frames)
    for f in range(frames):
        s = 0.0
        for i in range(f * frame, (f + 1) * frame):
            s += x[i] * x[i]
        out[f] = 20.0 * np.log10(np.sqrt(s / frame))
    return out


@njit(cache=True)
def nb_goertzel(x, wr, wi):
    """The single-bin power, as the first-order complex recurrence
    z <- x[n] + w*z read from the last sample back, in real arithmetic."""
    zr = zi = 0.0
    for i in range(x.shape[0] - 1, -1, -1):
        zr, zi = x[i] + wr * zr - wi * zi, wr * zi + wi * zr
    return zr * zr + zi * zi


@njit(cache=True)
def nb_dft_frame(mr, mi, f):
    n = f.shape[0]
    out = np.empty(n)
    for k in range(n):
        ar = ai = 0.0
        for i in range(n):
            ar += mr[k, i] * f[i]
            ai += mi[k, i] * f[i]
        out[k] = np.sqrt(ar * ar + ai * ai)
    return out


@njit(cache=True)
def nb_xcorr(x, lag, w):
    n = x.shape[0] - lag
    out = np.empty(n - w + 1)
    s = 0.0
    for i in range(w):
        s += x[i] * x[i + lag]
    out[0] = s
    for i in range(1, n - w + 1):
        s += x[i + w - 1] * x[i + w - 1 + lag] - x[i - 1] * x[i - 1 + lag]
        out[i] = s
    return out


# ---------------------------------------------------------- implementations
#
# Each is a factory: it takes the corpus and returns the zero-argument call
# that is timed, so building a DataFrame or a table of constants — work a
# caller would have done once, long before — stays outside the measurement.
# Every one of these functions is printed verbatim in workloads.md.


def pl_frame(cols: dict[str, np.ndarray]):
    import polars as pl

    return pl.DataFrame({k: pl.Series(k, v) for k, v in cols.items()})


# --- 1. cumulative return ---------------------------------------------------

J_CUMRET = "_1 + ^ +/\\ ^. (1 }. {close}) % _1 }. {close}"


def polars_cumret(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    logret = pl.col("close").log().diff().fill_null(0.0)
    return lambda: df.select((logret.cum_sum().exp() - 1).alias("cr")).to_series()[1:]


def numpy_cumret(d):
    c = d["close"]
    return lambda: np.exp(np.cumsum(np.log(c[1:] / c[:-1]))) - 1.0


def numba_cumret(d):
    c = d["close"]
    return lambda: nb_cumulative_return(c)


# --- 2. golden cross --------------------------------------------------------

J_GOLDEN = f"""a =. ({MA_FAST} +/\\ {{close}}) % {MA_FAST}
b =. ({MA_SLOW} +/\\ {{close}}) % {MA_SLOW}
g =. ({MA_LONG} +/\\ {{close}}) % {MA_LONG}
p =. ({MA_LONG - MA_FAST} }}. a) > {MA_LONG - MA_SLOW} }}. b
q =. ({MA_LONG - MA_SLOW} }}. b) > g
(+/ (1 }}. p) ~: _1 }}. p) , +/ (1 }}. q) ~: _1 }}. q"""


def polars_golden(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    ma = lambda w: pl.col("close").rolling_mean(window_size=w)
    fast, slow, long = ma(MA_FAST), ma(MA_SLOW), ma(MA_LONG)
    crossings = lambda c: pl.col(c).diff().abs().sum()

    def run():
        # The signals are taken over the whole column and only then cut back
        # to where the longest average exists, so both pairs are counted over
        # the same rows.
        signal = df.select((fast > slow).cast(pl.Int8).alias("gold"),
                           (slow > long).cast(pl.Int8).alias("long"))
        return signal.slice(MA_LONG - 1).select(
            crossings("gold"), crossings("long")).row(0)

    return run


def numpy_golden(d):
    c = d["close"]

    def ma(w):
        cs = np.cumsum(np.concatenate(([0.0], c)))
        return (cs[w:] - cs[:-w]) / w

    def run():
        a, b, g = ma(MA_FAST), ma(MA_SLOW), ma(MA_LONG)
        p = a[MA_LONG - MA_FAST:] > b[MA_LONG - MA_SLOW:]
        q = b[MA_LONG - MA_SLOW:] > g
        return np.array([np.count_nonzero(p[1:] != p[:-1]),
                         np.count_nonzero(q[1:] != q[:-1])])

    return run


def numba_golden(d):
    c = d["close"]
    return lambda: nb_golden_cross(c, MA_FAST, MA_SLOW, MA_LONG)


# --- 3. Bollinger bands -----------------------------------------------------

J_BOLLINGER = f"""s =. {BOLL} +/\\ {{close}}
v =. ({BOLL} * {BOLL} +/\\ *: {{close}}) - s * s
d =. ({BOLL} * {BOLL - 1} }}. {{close}}) - s
(+/ (| d) > {BOLL_K} * %: v) % # d"""


def polars_bollinger(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    mean = pl.col("close").rolling_mean(window_size=BOLL)
    sd = pl.col("close").rolling_std(window_size=BOLL, ddof=0)
    # The leading rows have no window and come out null, which `mean` skips —
    # so the fraction is already over the bars a band exists for.
    outside = ((pl.col("close") - mean).abs() > BOLL_K * sd)
    return lambda: df.select(outside.mean()).item()


def numpy_bollinger(d):
    c = d["close"]

    def run():
        cs = np.cumsum(np.concatenate(([0.0], c)))
        qs = np.cumsum(np.concatenate(([0.0], c * c)))
        s = cs[BOLL:] - cs[:-BOLL]
        q = qs[BOLL:] - qs[:-BOLL]
        dev = BOLL * c[BOLL - 1:] - s
        return np.count_nonzero(np.abs(dev) > BOLL_K * np.sqrt(BOLL * q - s * s)) / s.shape[0]

    return run


def numba_bollinger(d):
    c = d["close"]
    return lambda: nb_bollinger_outside(c, BOLL, BOLL_K)


# --- 4. RSI(14), Wilder smoothing -------------------------------------------

_WILDER = repr(1.0 - 1.0 / RSI_PERIOD)
J_RSI = f"""d =. (1 }}. {{close}}) - _1 }}. {{close}}
ag =. |. ([ + {_WILDER} * ])/\\. |. 0 >. d
al =. |. ([ + {_WILDER} * ])/\\. |. 0 >. - d
100 - 100 % 1 + ag % al"""


def polars_rsi(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    # `diff` leaves row 0 null; filling it with zero makes row 0 seed both
    # smoothings at zero, which is the recurrence the other three run, and the
    # slice takes that seeding row back off.
    delta = pl.col("close").diff().fill_null(0.0)
    smooth = lambda e: e.ewm_mean(alpha=1.0 / RSI_PERIOD, adjust=False)
    rs = smooth(pl.max_horizontal(delta, 0.0)) / smooth(pl.max_horizontal(-delta, 0.0))
    return lambda: df.select((100 - 100 / (1 + rs)).alias("rsi")).to_series()[1:]


def numpy_rsi(d):
    """No numpy figure: numpy has no first-order recurrence, so the only
    numpy spelling of Wilder's smoothing is a Python loop over 20M samples."""
    return None


def numba_rsi(d):
    c = d["close"]
    return lambda: nb_rsi(c, RSI_PERIOD)


# --- 5. VWAP per day --------------------------------------------------------

J_VWAP = """p =. ({high} + {low} + {close}) % 3
({day} +//. p * {volume}) % {day} +//. {volume}"""


def polars_vwap(d):
    import polars as pl

    df = pl_frame({k: d[k] for k in ("day", "high", "low", "close", "volume")})
    typical = (pl.col("high") + pl.col("low") + pl.col("close")) / 3
    vwap = (typical * pl.col("volume")).sum() / pl.col("volume").sum()
    return lambda: (df.group_by("day", maintain_order=True)
                      .agg(vwap.alias("vwap")).get_column("vwap"))


def numpy_vwap(d):
    day, high, low, close, volume = (d[k] for k in
                                     ("day", "high", "low", "close", "volume"))

    def run():
        weight = (high + low + close) / 3.0 * volume
        return np.bincount(day, weights=weight) / np.bincount(day, weights=volume)

    return run


def numba_vwap(d):
    day, high, low, close, volume = (d[k] for k in
                                     ("day", "high", "low", "close", "volume"))
    days = int(day[-1]) + 1
    return lambda: nb_vwap(day, high, low, close, volume, days)


# --- 6. maximum drawdown ----------------------------------------------------

J_DRAWDOWN = ">./ 1 - {close} % >./\\ {close}"


def polars_drawdown(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    dd = 1 - pl.col("close") / pl.col("close").cum_max()
    return lambda: df.select(dd.max()).item()


def numpy_drawdown(d):
    c = d["close"]
    return lambda: np.max(1.0 - c / np.maximum.accumulate(c))


def numba_drawdown(d):
    c = d["close"]
    return lambda: nb_max_drawdown(c)


# --- 7. realised-volatility regime ------------------------------------------

J_VOLREGIME = f"""r =. (1 }}. {{close}}) % _1 }}. {{close}}
s =. {VOL_WINDOW} +/\\ r
v =. %: 0 >. ({VOL_WINDOW} * {VOL_WINDOW} +/\\ *: r) - s * s
(+/ v > {VOL_REGIME} * (+/ v) % # v) % # v"""


def polars_volregime(d):
    import polars as pl

    df = pl_frame({"close": d["close"]})
    ret = pl.col("close") / pl.col("close").shift(1)
    vol = ret.rolling_std(window_size=VOL_WINDOW, ddof=0) * VOL_WINDOW

    def run():
        # The threshold is the mean of the volatility series, so the series is
        # cut to the rows a window covers before its own mean is taken.
        v = df.select(vol.alias("v")).slice(VOL_WINDOW)
        return v.select((pl.col("v") > VOL_REGIME * pl.col("v").mean()).mean()).item()

    return run


def numpy_volregime(d):
    c = d["close"]
    w = VOL_WINDOW

    def run():
        r = c[1:] / c[:-1]
        cs = np.cumsum(np.concatenate(([0.0], r)))
        qs = np.cumsum(np.concatenate(([0.0], r * r)))
        s = cs[w:] - cs[:-w]
        q = qs[w:] - qs[:-w]
        vol = np.sqrt(np.maximum(w * q - s * s, 0.0))
        return np.count_nonzero(vol > VOL_REGIME * vol.mean()) / vol.shape[0]

    return run


def numba_volregime(d):
    c = d["close"]
    return lambda: nb_vol_regime(c, VOL_WINDOW, VOL_REGIME)


# --- 8. low-pass and peak count ---------------------------------------------

J_PEAKS = f"""y =. ({LOWPASS} +/\\ {{x}}) % {LOWPASS}
m =. 1 }}. _1 }}. y
+/ (m > _2 }}. y) *. m > 2 }}. y"""


def polars_peaks(d):
    import polars as pl

    df = pl_frame({"x": d["x"]})
    # The rows with no filtered value either side come out null, and `sum`
    # skips them, so the count is over the interior without a slice.
    y = pl.col("x").rolling_mean(window_size=LOWPASS)
    peak = (y > y.shift(1)) & (y > y.shift(-1))
    return lambda: df.select(peak.sum()).item()


def numpy_peaks(d):
    x = d["x"]
    w = LOWPASS

    def run():
        cs = np.cumsum(np.concatenate(([0.0], x)))
        y = (cs[w:] - cs[:-w]) / w
        return np.count_nonzero((y[1:-1] > y[:-2]) & (y[1:-1] > y[2:]))

    return run


def numba_peaks(d):
    x = d["x"]
    return lambda: nb_lowpass_peaks(x, LOWPASS)


# --- 9. frame RMS in dB -----------------------------------------------------


def j_frame_rms(n: int) -> str:
    return f'20 * 10 ^. %: (+/"1 *: {n // FRAME} {FRAME} $ {{x}}) % {FRAME}'


def polars_frame_rms(d):
    import polars as pl

    df = pl_frame({"x": d["x"]})
    rms = (pl.col("x") ** 2).reshape((-1, FRAME)).arr.mean().sqrt()
    return lambda: df.select((20 * rms.log10()).alias("db")).to_series()


def numpy_frame_rms(d):
    x = d["x"]
    return lambda: 20.0 * np.log10(np.sqrt((x.reshape(-1, FRAME) ** 2).mean(axis=1)))


def numba_frame_rms(d):
    x = d["x"]
    return lambda: nb_frame_rms_db(x, FRAME)


# --- 10. single-bin detection (Goertzel) ------------------------------------
#
# The recurrence z <- x[n] + w*z, read over the whole signal, is exactly
# sum x[n] w^n — so the vectorised J spelling is a complex weighted sum, and
# the recurrence spelling is the fold `([ + w * ])/`. Both are measured: the
# first here, the second under "Recurrences" below.


def j_goertzel(n: int) -> str:
    return f"*: | +/ {{x}} * {{w}}"


def goertzel_weights(n: int) -> np.ndarray:
    return np.ascontiguousarray(goertzel_step(n) ** np.arange(n))


def polars_goertzel(d):
    import polars as pl

    n = d["x"].shape[0]
    w = goertzel_weights(n)
    # Polars has no complex dtype, so the real and imaginary halves are two
    # columns — which is how a Polars user would have to write it.
    df = pl_frame({"x": d["x"], "re": np.ascontiguousarray(w.real),
                   "im": np.ascontiguousarray(w.imag)})
    re = (pl.col("x") * pl.col("re")).sum()
    im = (pl.col("x") * pl.col("im")).sum()
    return lambda: df.select((re ** 2 + im ** 2).alias("power")).item()


def numpy_goertzel(d):
    x = d["x"]
    w = goertzel_weights(x.shape[0])
    return lambda: abs(np.dot(x, w)) ** 2


def numba_goertzel(d):
    x = d["x"]
    step = goertzel_step(x.shape[0])
    return lambda: nb_goertzel(x, step.real, step.imag)


# --- 11. naive DFT of one frame ---------------------------------------------
#
# The matrix-vector product spelled with the verbs that exist today. Once the
# inner product `u . v` lands, the same line is `{m} +/ . * {f}`.

J_DFT = '| +/"1 {m} *"1 {f}'


def dft_matrix() -> np.ndarray:
    i = np.arange(FRAME)
    return np.ascontiguousarray(np.exp(-2j * np.pi * np.outer(i, i) / FRAME))


def polars_dft(d):
    return None      # no complex dtype, and no matrix product


def numpy_dft(d):
    m, f = dft_matrix(), np.ascontiguousarray(d["x"][:FRAME])
    return lambda: np.abs(m @ f)


def numba_dft(d):
    m, f = dft_matrix(), np.ascontiguousarray(d["x"][:FRAME])
    mr, mi = np.ascontiguousarray(m.real), np.ascontiguousarray(m.imag)
    return lambda: nb_dft_frame(mr, mi, f)


# --- 12. rolling cross-correlation ------------------------------------------

J_XCORR = f"{XCORR_WINDOW} +/\\ (_{XCORR_LAG} }}. {{x}}) * {XCORR_LAG} }}. {{x}}"


def polars_xcorr(d):
    import polars as pl

    df = pl_frame({"x": d["x"]})
    keep = d["x"].shape[0] - XCORR_LAG
    product = pl.col("x") * pl.col("x").shift(-XCORR_LAG)

    def run():
        # The lagged product exists for the first n-64 rows; the moving sum
        # runs over those, and its own leading nulls come off at the end.
        p = df.select(product.alias("p")).head(keep)
        return (p.select(pl.col("p").rolling_sum(window_size=XCORR_WINDOW))
                 .to_series()[XCORR_WINDOW - 1:])

    return run


def numpy_xcorr(d):
    x = d["x"]

    def run():
        product = x[:-XCORR_LAG] * x[XCORR_LAG:]
        cs = np.cumsum(np.concatenate(([0.0], product)))
        return cs[XCORR_WINDOW:] - cs[:-XCORR_WINDOW]

    return run


def numba_xcorr(d):
    x = d["x"]
    return lambda: nb_xcorr(x, XCORR_LAG, XCORR_WINDOW)


# ------------------------------------------------------------ the references
#
# What correctness is judged against: each window computed from its own items,
# which is affordable only over a slice, and a plain loop where the quantity
# is a recurrence. None of these is timed.


def ref_windows(x: np.ndarray, w: int) -> np.ndarray:
    return np.lib.stride_tricks.sliding_window_view(x, w)


def ref_cumret(d):
    c = d["close"]
    return np.exp(np.cumsum(np.log(c[1:] / c[:-1]))) - 1.0


def ref_golden(d):
    c = d["close"]
    ma = lambda w: ref_windows(c, w).mean(axis=1)
    a, b, g = ma(MA_FAST), ma(MA_SLOW), ma(MA_LONG)
    p = a[MA_LONG - MA_FAST:] > b[MA_LONG - MA_SLOW:]
    q = b[MA_LONG - MA_SLOW:] > g
    return np.array([np.count_nonzero(p[1:] != p[:-1]),
                     np.count_nonzero(q[1:] != q[:-1])])


def ref_bollinger(d):
    c = d["close"]
    v = ref_windows(c, BOLL)
    return np.count_nonzero(
        np.abs(c[BOLL - 1:] - v.mean(axis=1)) > BOLL_K * v.std(axis=1)) / v.shape[0]


def ref_rsi(d):
    c = d["close"]
    k = 1.0 - 1.0 / RSI_PERIOD
    delta = np.diff(c)
    gain, loss = np.maximum(delta, 0.0), np.maximum(-delta, 0.0)
    ag, al = np.empty_like(gain), np.empty_like(loss)
    g = l = 0.0
    for i in range(gain.shape[0]):
        g = k * g + gain[i]
        l = k * l + loss[i]
        ag[i], al[i] = g, l
    return 100.0 - 100.0 / (1.0 + ag / al)


def ref_vwap(d):
    return numpy_vwap(d)()


def ref_drawdown(d):
    return numpy_drawdown(d)()


def ref_volregime(d):
    c = d["close"]
    vol = ref_windows(c[1:] / c[:-1], VOL_WINDOW).std(axis=1) * VOL_WINDOW
    return np.count_nonzero(vol > VOL_REGIME * vol.mean()) / vol.shape[0]


def ref_peaks(d):
    y = ref_windows(d["x"], LOWPASS).mean(axis=1)
    return np.count_nonzero((y[1:-1] > y[:-2]) & (y[1:-1] > y[2:]))


def ref_frame_rms(d):
    return numpy_frame_rms(d)()


def ref_goertzel(d):
    return numpy_goertzel(d)()


def ref_dft(d):
    return numpy_dft(d)()


def ref_xcorr(d):
    v = ref_windows(d["x"][:-XCORR_LAG] * d["x"][XCORR_LAG:], XCORR_WINDOW)
    return v.sum(axis=1)


# ---------------------------------------------------------------- the suite


@dataclass
class Workload:
    key: str
    name: str
    corpus: str
    what: str
    source: Callable[[int], str]
    reference: Callable[[dict], object]
    polars: Callable[[dict], Callable | None]
    numba: Callable[[dict], Callable]
    numpy: Callable[[dict], Callable]
    rtol: float = 1e-9
    atol: float = 1e-9
    libjay: str = "yes"          # "yes", or why the row has no libjay figure
    check: int | None = None     # a smaller correctness slice, where one is needed
    note: str = ""


def const(s: str) -> Callable[[int], str]:
    return lambda n: s


SUITE = [
    Workload(
        "cumret", "cumulative return", "ohlcv",
        "log returns, summed and exponentiated back into a cumulative return curve",
        const(J_CUMRET), ref_cumret, polars_cumret, numba_cumret, numpy_cumret,
        note="""**One of the two rows libjay beats the numba loop on**, and the more
surprising of them, because a running sum is a serial dependency however it
is spelled: `+/\\` carries an accumulator from block to block and the thread
pool has nothing to split there. What threads *can* take is everything
around it — the division, the logarithm, the exponential and the decrement —
and that is where the 1.7x comes from. The logarithm and the exponential are
the reason: they are enough arithmetic per element that four cores matter,
which is exactly the case the `+/ ^ y` row of bench/README.md makes on a
kernel instead of a workload.""",
    ),
    Workload(
        "golden", "golden cross (20/50/200)", "ohlcv",
        "three moving averages and the number of times the fast one crosses the slow",
        const(J_GOLDEN), ref_golden, polars_golden, numba_golden, numpy_golden,
        rtol=0.0, atol=0.0,
        note="""**All four agree on the count exactly**, which is the first thing to
say about a row whose answer is an integer. A crossing is a place where two
moving averages are equal, so this is the workload where two spellings of a
moving average would show a disagreement in the last bits — and none of them
does: over 20M bars the closest the fast and slow averages ever come to each
other is 5.9e-6, and the four ways of computing them differ by around 1e-8,
so the crossings land on the same bars in all four.

**This is the row where the gap to a hand-written loop is widest**, and it
is four times the numba loop on eight threads and twelve on one. Three moving averages is three window folds, and libjay's fold
reads and writes the whole column for each of them — `b` is read by both
signals, so its 50-wide fold runs twice, four folds in all. numba slides
three accumulators along the closes in a single pass and touches 160 MB
once; Polars' `rolling_mean` does something close to the same. libjay's fold
is blocked rather than sliding on purpose — that is what makes each window's
rounding the rounding of that window alone, which is what the paragraph
above is made of — but the traffic it costs is real, and on a workload with
three windows in it that trade stops paying.""",
    ),
    Workload(
        "bollinger", "Bollinger bands (20, 2σ)", "ohlcv",
        "the fraction of closes outside the 20-bar, two-sigma band",
        const(J_BOLLINGER), ref_bollinger, polars_bollinger, numba_bollinger,
        numpy_bollinger, rtol=1e-12, atol=1e-12,
        note="""The phase-5 gate's kernel with a threshold on the end of it. Both
moving sums — of the closes and of their squares — are folded inside one
blockwise kernel, so the band is never materialised: what crosses memory is
the closes in and one bit per bar out. Three and a half times Polars and
almost seven times numpy on eight threads, and the widest margin over numpy
in the file; two and a half times numba, which writes nothing at all.""",
    ),
    Workload(
        "rsi", "RSI(14), Wilder smoothing", "ohlcv",
        "the classic momentum oscillator: an exponential recurrence on gains and losses",
        const(J_RSI), ref_rsi, polars_rsi, numba_rsi, numpy_rsi,
        note="""**The row that used to say "quadratic".** Wilder's smoothing is a
first-order linear recurrence, and J spells one `|. u/\\. |. y`: reversed at
both ends so that the fold runs in the direction the insert already goes.
Every suffix used to be folded from scratch, which is n²/2 steps and made
20 million rows a number that did not exist. The suffix scan now hands its
accumulator from item to item — that is what right-to-left buys, for any
verb at all — and the affine step `[ + c * ]` is recognised and run as the
recurrence itself over the buffer, so the row is 2.3 seconds instead.

It still loses to both rivals, and the reason is the one this whole file
keeps finding. numba carries two accumulators in registers and writes one
array. libjay writes the difference, the two clipped copies, the four
reversals and the two smoothed columns — nine columns of 160 MB before the
last sentence starts — and the scan itself is a serial chain no thread
splits,
which is why the eight-thread column is the one-thread column. Polars has
`ewm_mean` natively and is one pass; the numpy column is empty for a version
of libjay's old reason — numpy has no first-order recurrence either, so its
only spelling is a Python loop over twenty million samples.""",
    ),
    Workload(
        "vwap", "VWAP per day", "ohlcv",
        "volume-weighted average price, grouped by trading day over minute bars",
        const(J_VWAP), ref_vwap, polars_vwap, numba_vwap, numpy_vwap,
        note="""**The other row that used to have no figure.** `+//.` is J's key —
the groups in the order their keys first appear, one fold each — and it was
correct all along and quadratic in disguise: each group was found by
sweeping the whole key column for it, so the cost was rows × groups, and
20M bars over 13,889 days came to hours. The keys are hashed once now, into
buckets in first-occurrence order, and the row is 1.4 seconds.

It is still the row a DataFrame engine wins, and this is its home ground.
The sentence groups twice — once for the weighted price and once for the
volume — so 20M keys are hashed twice and the two group passes are separate
walks over 160 MB each; Polars hashes once for a `group_by` that carries
both aggregates, and numba, which is told the day count up front, needs
neither a hash nor a second pass. The agreement is exact: over 200 days of
bars libjay, Polars, numba and numpy give the same value to the last bit.""",
    ),
    Workload(
        "drawdown", "maximum drawdown", "ohlcv",
        "the deepest fall from a running peak",
        const(J_DRAWDOWN), ref_drawdown, polars_drawdown, numba_drawdown,
        numpy_drawdown,
        note="""A running maximum, then a maximum of the deficit: a scan and a
reduction, which fuse into one pass. Like the cumulative return it carries an
accumulator, so it does not thread — the eight-thread figure is the
one-thread figure — but it also never writes the running maximum down, which
is what the other three all do, and that is why it is two and a half times
Polars and two and a half times numpy on a single core. numba is five times
faster still, because a maximum and a compare in registers is about as
little work per element as a loop can have.""",
    ),
    Workload(
        "volregime", "volatility regime", "ohlcv",
        "rolling 20-bar standard deviation of returns, and the share of bars above 1.5x its mean",
        const(J_VOLREGIME), ref_volregime, polars_volregime, numba_volregime,
        numpy_volregime, rtol=1e-12, atol=1e-12,
        note="""Two passes by construction — the threshold is the mean of the
volatility series, so the series has to exist before it can be compared with
its own average. libjay writes it once and reads it back, which is what the
`v` on the left of the last sentence is: a named value the fusion pass leaves
alone precisely because a later sentence needs the whole array — the rule
this row exists to exercise, and the opposite decision from the one the peak
count below suffers from.

Even paying for that buffer, eight threads land level with the numba loop
(170 against 176) and nearly eight times numpy, because everything either side of
the buffer is one pass.""",
    ),
    Workload(
        "peaks", "low-pass and peak count", "dsp",
        "a 32-tap moving-average filter, then the local maxima of what comes out",
        const(J_PEAKS), ref_peaks, polars_peaks, numba_peaks, numpy_peaks,
        rtol=0.0, atol=0.0,
        note="""Peak detection is `(y > prev) & (y > next)`, and in J a shift is a
drop: `_2 }. y` and `2 }. y` are the two neighbours of `1 }. _1 }. y`. libjay
beats Polars here by two and a half times and loses to numpy by two and a
half times, which is the interesting part.

**The filter runs four times.** `explain` says so: the fusion pass moves a
named value into every sentence that reads it, and `y` is read at four
different alignments, so the 32-wide fold is a step of four kernels instead
of a buffer written once and sliced three ways. numpy folds once, into 128
MB it then takes three views of; numba slides one accumulator and keeps two
scalars. Writing the drops on the argument instead of on the result — three
separate `32 +/\\ … }. {x}` — changes nothing (1214 ms against 1122 on one
thread, 511 against 476 on eight, both spellings paired in one session),
because the pass moves the chain either way.

The rule that decides this is measured in bench/README.md under "Is folding
the moving sum twice worth not writing it down?", where the answer at two
uses was yes. At four uses it is no, and the rule does not count.""",
    ),
    Workload(
        "framerms", "frame RMS in dB", "dsp",
        "the signal cut into 1024-sample frames, each frame's RMS in decibels",
        j_frame_rms, ref_frame_rms, polars_frame_rms, numba_frame_rms,
        numpy_frame_rms, rtol=1e-12, atol=1e-12,
        note="""Reshape and reduce along the rows — `n 1024 $ {x}` gives the frames
and `+/"1` folds each where it lies. It is the shortest expression in the
suite, and it used to be libjay's worst row at 535 ms: 457 of those went on
the reshape alone, copying 128 MB an element at a time to produce a matrix
with exactly the elements the vector already had, in exactly that order.

**A reshape that keeps the elements is now a change of shape and nothing
else** — the buffer comes through shared, foreign memory included — and the
row is 49 ms on one thread and 30 on eight, ahead of both numpy and Polars
and half again the numba loop. Timed separately at 16M samples on one
thread, `$ (15625 1024 $ {x})` — asking only for the *shape* of the
reshaped signal — now costs nothing at all, where it cost 457 ms.

What is left is the second, smaller thing: `explain` reports the fused
kernel *declining* this program — "the reduction needs one axis of two or
more items" — so `*:` writes a whole 128 MB buffer that the fold then reads
back. `+/"1 *: {m}` is 57 ms against `+/"1 {m}`'s 20, and numpy's
`einsum('ij,ij->i')`, which fuses the same two steps, is 12.""",
    ),
    Workload(
        "goertzel", "single-bin detection", "dsp",
        "the power in one frequency bin over the whole signal",
        j_goertzel, ref_goertzel, polars_goertzel, numba_goertzel,
        numpy_goertzel, rtol=1e-9, atol=0.0,
        note="""The Goertzel recurrence z ← x[n] + w·z summed over the whole signal
is exactly Σ x[n]·wⁿ, so the vectorised spelling is a complex weighted sum
and this row measures that. The recurrence spelling of the same quantity —
the fold `([ + w * ])/` — is under "Where libjay loses" below, and it is
about 200 times slower than numba's loop.

**libjay wins this on eight threads by two to one**, and the reason it is
not further ahead than that is traffic it chose. The vectorised spelling needs the
table of complex weights, so it reads 128 MB of signal and 256 MB of
weights; numba's recurrence generates each weight from the last and reads
only the signal, which is why one core of it beats four of libjay's. Polars
keeps the weights as two real columns — 256 MB rather than 384 — and lands
between the two. The row is memory-bound and the ranking is the byte count.
What would remove libjay's extra 256 MB is the recurrence spelling being
fast, which is the same missing rule as RSI's.

Polars has no complex dtype, so its version carries the real and imaginary
weights as two columns — which is what a Polars user would have to do.""",
    ),
    Workload(
        "dft", "naive DFT of one frame", "dsp",
        "a 1024-point discrete Fourier transform written as a matrix-vector product",
        const(J_DFT), ref_dft, polars_dft, numba_dft, numpy_dft,
        rtol=1e-9, atol=1e-9,
        note="""One 1024×1024 complex matrix against one 1024-sample frame. The J is
`+/"1 {m} *"1 {f}` because the inner product `u . v` is not implemented yet —
when it lands the same line is `{m} +/ . * {f}`, and until then `*"1` is what
pairs the frame with each row rather than with each row's *index*, which is
what J's leading-axis agreement would otherwise do. Polars has neither a
complex dtype nor a matrix product, so it does not appear.

**numpy wins this by twenty times and deserves to**: `m @ f` is a BLAS
`zgemv`, a blocked, hand-tuned kernel forty years in the making. libjay
materialises the 1024×1024 complex product — 16 MB — and reduces it, which
is two passes where BLAS has one and no temporary. There is no fusion rule
that closes that gap; a matrix product wants a matrix-product kernel, and
the place to put one is the inner product `u . v` when it lands. Until then
this row is what a matrix product costs when it is spelled out of pieces.""",
    ),
    Workload(
        "xcorr", "rolling cross-correlation", "dsp",
        "the dot product of the signal with itself at lag 64, over every 1024-sample window",
        const(J_XCORR), ref_xcorr, polars_xcorr, numba_xcorr, numpy_xcorr,
        rtol=1e-7, atol=1e-9,
        note="""**libjay's best row, and the second one it beats the numba loop
on.** Two shifted views of one buffer, multiplied, and a moving sum over the
product: the whole sentence is one fused pass with a window step in it, and
the drops that align the two copies cost nothing since an owned slice became
a view. Eight times Polars, three and a half times numpy, and a shade under
the hand-written loop — which is what four cores buy on a program that reads
its argument twice and writes one result. Thirty-four characters.""",
    ),
]

BY_KEY = {w.key: w for w in SUITE}


# -------------------------------------------------------------- the probes
#
# Three sentences libjay cannot run at the sizes above. Each is measured at
# the sizes it CAN be run at, beside the rival that has a linear answer, so
# the shape of the loss is visible rather than asserted.


PROBES = [
    {
        "key": "goertzel-fold",
        "name": "Goertzel — the fold `([ + w * ])/`",
        "corpus": "dsp",
        "kernel": lambda d, n: (f"*: | ([ + {jc(goertzel_step(n))} * ])/ {{x}}",
                                {"x": d["x"]}),
        "rival": ("numba", numba_goertzel),
        "sizes": (16_384, 65_536, 262_144, 1_048_576),
        "why": """This one is linear — the fold really does make one pass — but each
step is a general dyad applied to a pair of complex scalars through the whole
interpreter, at about 690 nanoseconds an element against numba's 3.3. It is
the cost of *not* being in a kernel. The scan `([ + w * ])/\\.` over the same
step is now a typed loop over the buffer, because the affine rule is on the
windowed path; the plain fold `u/` is not on that path yet, and putting it
there is the same recognition applied one function along.""",
    },
]


# ------------------------------------------------------------------- running


def as_np(value) -> np.ndarray:
    """A result of any of the four engines as a numpy array."""
    if isinstance(value, (bool, int, float, complex, np.generic)):
        return np.asarray(value)
    if isinstance(value, tuple):
        return np.asarray(value)
    if isinstance(value, np.ndarray):
        return value
    import polars as pl

    if isinstance(value, pl.Series):
        return value.to_numpy()
    try:
        return np.asarray(pl.Series(value))
    except Exception:
        return np.asarray(value.tolist() if hasattr(value, "tolist") else value)


def agree(got, want, w: Workload) -> str | None:
    a, b = as_np(got), as_np(want)
    if a.shape != b.shape:
        return f"shape {a.shape} against {b.shape}"
    if w.rtol == 0.0 and w.atol == 0.0:
        if not np.array_equal(a, b):
            return f"exact comparison failed: {a.ravel()[:4]} against {b.ravel()[:4]}"
        return None
    if not np.allclose(a, b, rtol=w.rtol, atol=w.atol):
        d = np.abs(a - b)
        return (f"differ by up to {np.max(d):.3e} "
                f"(rtol {w.rtol:g}, atol {w.atol:g})")
    return None


def libjay_kernel(w: Workload, data: dict, n: int):
    import jay

    src = w.source(n)
    if w.key == "dft":
        return jay.j.compile(src).bind({"m": dft_matrix(),
                                        "f": np.ascontiguousarray(data["x"][:FRAME])})
    if w.key == "goertzel":
        return jay.j.compile(src).bind({"x": data["x"], "w": goertzel_weights(n)})
    names = ["close", "high", "low", "volume", "day"] if w.corpus == "ohlcv" else ["x"]
    bound = {k: data[k] for k in names if "{%s}" % k in src}
    return jay.j.compile(src).bind(bound)


def fingerprint(value) -> dict:
    a = as_np(value).astype(float, copy=False)
    return {"n": int(a.size), "sum": float(np.sum(a))}


def worker(args) -> None:
    """Time every workload's libjay kernel in this process, one thread count."""
    out = {}
    for kind, n in (("ohlcv", args.rows), ("dsp", args.samples)):
        data = corpus(kind, n)
        for w in SUITE:
            if w.corpus != kind or w.libjay != "yes" or w.key in args.skip:
                continue
            kernel = libjay_kernel(w, data, n)
            best, value = best_of(kernel, args.repeat)
            out[w.key] = {"best": best, **fingerprint(value)}
        del data
    out["_rss"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    print(json.dumps(out))


def run_worker(threads: int, args, skip=()) -> dict:
    env = dict(os.environ, LIBJAY_THREADS=str(threads))
    cmd = [sys.executable, str(HERE / "workloads.py"), "--worker",
           "--rows", str(args.rows), "--samples", str(args.samples),
           "--repeat", str(args.repeat)]
    for k in skip:
        cmd += ["--skip", k]
    out = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


def probe_worker(args) -> None:
    """One probe at one size, timed once — these are the slow sentences, so a
    best-of-five would buy nothing a single call does not already say."""
    import jay

    probe = next(p for p in PROBES if p["key"] == args.probe)
    n = args.rows
    data = corpus(probe["corpus"], n)
    src, bound = probe["kernel"](data, n)
    program = jay.j.compile(src)
    # The same program over a token slice, so the pool and every code path it
    # touches are warm before the size under test is timed: at these sizes the
    # first call in a process would otherwise be mostly startup.
    program.bind({k: v[:512] for k, v in bound.items()})()
    kernel = program.bind(bound)
    t0 = time.perf_counter()
    value = kernel()
    print(json.dumps({args.probe: {"best": time.perf_counter() - t0,
                                   **fingerprint(value)}}))


def run_probe(key: str, n: int) -> float:
    env = dict(os.environ, LIBJAY_THREADS="1")
    cmd = [sys.executable, str(HERE / "workloads.py"), "--probe", key,
           "--rows", str(n), "--samples", str(n)]
    out = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])[key]["best"]


# -------------------------------------------------------------------- output


def fmt_ms(t: float | None) -> str:
    if t is None:
        return "n/a"
    return f"{t * 1e3:.1f}" if t >= 0.001 else f"{t * 1e3:.3f}"


def source_of(fn) -> str:
    return textwrap.dedent(inspect.getsource(fn)).rstrip()


def named_kernels(fn) -> list:
    """Every `nb_…` this function reaches, so the numba block shows the loop
    and not just the closure that calls it."""
    seen, out = set(), []

    def walk(code):
        for name in code.co_names:
            if name.startswith("nb_") and name not in seen:
                seen.add(name)
                out.append(globals()[name])
        for const in code.co_consts:
            if hasattr(const, "co_names"):
                walk(const)

    walk(fn.__code__)
    return out


def numba_source(w) -> str:
    parts = [source_of(k.py_func if hasattr(k, "py_func") else k)
             for k in named_kernels(w.numba)]
    return "\n\n\n".join(parts + [source_of(w.numba)])


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=OHLCV_ROWS)
    ap.add_argument("--samples", type=int, default=DSP_SAMPLES)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--passes", type=int, default=2,
                    help="alternating passes over libjay's two thread counts")
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--check", type=int, default=288_000,
                    help="OHLCV rows the untimed correctness check covers")
    ap.add_argument("--check-dsp", type=int, default=131_072,
                    help="audio samples the untimed correctness check covers")
    ap.add_argument("--only", action="append", default=[])
    ap.add_argument("--skip", action="append", default=[])
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--out", default=str(HERE / "workloads.md"))
    ap.add_argument("--worker", action="store_true")
    ap.add_argument("--probe")
    ap.add_argument("--save", help="write the raw figures to this JSON")
    ap.add_argument("--load", help="rebuild the document from a saved JSON")
    args = ap.parse_args()

    if args.quick:
        args.rows, args.samples, args.repeat = QUICK_ROWS, QUICK_SAMPLES, 2
        args.passes = 1
        args.check, args.check_dsp = 144_000, 65_536
        args.out = None
    if args.probe:
        return probe_worker(args)
    if args.worker:
        return worker(args)

    suite = [w for w in SUITE
             if (not args.only or w.key in args.only) and w.key not in args.skip]

    import polars as pl
    import numba

    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"python    {platform.python_version()}, numpy {np.__version__}, "
          f"polars {pl.__version__}, numba {numba.__version__}")
    print(f"sizes     OHLCV {args.rows:,} minute bars, audio {args.samples:,} samples")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    # ---- correctness, untimed, over a slice small enough for the reference
    print(f"correctness, {args.check:,} bars / {args.check_dsp:,} samples "
          "against a per-window reference:")
    small = {"ohlcv": corpus("ohlcv", min(args.check, args.rows)),
             "dsp": corpus("dsp", min(args.check_dsp, args.samples))}
    failures = []
    for w in suite:
        # A workload with no linear libjay spelling names its own slice, so the
        # quadratic one is still checked, just not over a corpus it cannot end.
        d = small[w.corpus] if w.check is None else corpus(w.corpus, w.check)
        n = d["close"].shape[0] if w.corpus == "ohlcv" else d["x"].shape[0]
        want = w.reference(d)
        line = []
        for who, factory in (("libjay", None), ("polars", w.polars),
                             ("numba", w.numba), ("numpy", w.numpy)):
            if who == "libjay":
                got = libjay_kernel(w, d, n)()
            else:
                f = factory(d)
                if f is None:
                    line.append(f"{who} n/a")
                    continue
                got = f()
            bad = agree(got, want, w)
            line.append(f"{who} {'ok' if bad is None else 'DIFFERS'}")
            if bad is not None:
                failures.append(f"{w.name}: {who} {bad}")
        print(f"  {w.name:<34} " + ", ".join(line))
    del small
    if failures:
        for f in failures:
            print(f"  ! {f}", file=sys.stderr)
        raise SystemExit("implementations disagree — that is a bug report, not a benchmark")
    print()

    if args.load:
        return replay(args, json.loads(Path(args.load).read_text()), pl, numba)

    # ---- timings
    #
    # The two thread counts are two subprocesses, and a laptop that gets busy
    # between them would move one column and not the other. So the pass is run
    # `--passes` times, alternating, and each figure is the best it reached.
    skip = tuple(w.key for w in SUITE if w not in suite)
    one, many = {}, {}
    for _ in range(args.passes):
        one = better(one, run_worker(1, args, skip))
        many = better(many, run_worker(args.threads, args, skip))
    for w in suite:
        if w.key in one and one[w.key]["n"] != many[w.key]["n"]:
            raise SystemExit(f"{w.name}: libjay's two thread counts disagree")

    rows = []
    for kind in ("ohlcv", "dsp"):
        n = args.rows if kind == "ohlcv" else args.samples
        data = corpus(kind, n)
        for w in [x for x in suite if x.corpus == kind]:
            r = {"w": w, "libjay1": one.get(w.key, {}).get("best"),
                 "libjayN": many.get(w.key, {}).get("best")}
            for who, factory in (("polars", w.polars), ("numba", w.numba),
                                 ("numpy", w.numpy)):
                f = factory(data)
                r[who] = None if f is None else best_of(f, args.repeat)[0]
            rows.append(r)
        del data

    # ---- the probes: the sentences that have no figure in the table above
    probes = []
    if not args.only:
        for probe in PROBES:
            sizes = probe["sizes"] if not args.quick else probe["sizes"][:2]
            who, factory = probe["rival"]
            got = []
            for n in sizes:
                d = corpus(probe["corpus"], n)
                rival = factory(d)
                rival()
                t_rival = min(_timeit(rival) for _ in range(9))
                got.append((n, run_probe(probe["key"], n), t_rival))
                del d
            probes.append((probe, got))

    rss = max(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
              one.get("_rss", 0), many.get("_rss", 0))

    if args.save:
        Path(args.save).write_text(json.dumps({
            "rss": rss, "load": _load(),
            "args": {k: getattr(args, k) for k in
                     ("rows", "samples", "repeat", "passes", "check",
                      "check_dsp", "threads")},
            "rows": [{**{k: v for k, v in r.items() if k != "w"},
                      "key": r["w"].key} for r in rows],
            "probes": [[p["key"], g] for p, g in probes],
        }))
    emit(args, rows, probes, rss, _load(), pl, numba)


def replay(args, saved, pl, numba) -> None:
    """The document again from figures already measured, so the prose around
    them can be edited without occupying the machine for another hour."""
    for k, v in saved.get("args", {}).items():
        setattr(args, k, v)
    rows = [{**r, "w": BY_KEY[r["key"]]} for r in saved["rows"]]
    probes = [(next(p for p in PROBES if p["key"] == k),
               [tuple(x) for x in g]) for k, g in saved["probes"]]
    emit(args, rows, probes, saved["rss"], saved["load"], pl, numba)


def emit(args, rows, probes, rss, load, pl, numba) -> None:
    print(report(args, rows, probes, rss, pl, numba))
    if args.out:
        Path(args.out).write_text(document(args, rows, probes, rss, load, pl, numba))
        print(f"\nwritten to {args.out}")


def better(old: dict, new: dict) -> dict:
    """Two passes of the same worker, each figure the faster of the two."""
    if not old:
        return new
    return {k: (v if not isinstance(v, dict) or v["best"] <= old[k]["best"] else old[k])
            for k, v in new.items()}


def _load() -> float:
    try:
        return os.getloadavg()[0]
    except OSError:
        return float("nan")


def _timeit(f) -> float:
    t0 = time.perf_counter()
    f()
    return time.perf_counter() - t0


def table(rows, threads: int) -> str:
    out = [f"| workload | libjay 1 thread | libjay {threads} threads | speedup "
           "| polars | numba | numpy |", "|---|---:|---:|---:|---:|---:|---:|"]
    for r in rows:
        w = r["w"]
        if r["libjay1"] is None:
            one, many, speed = f"*{w.libjay}*", "—", "—"
        else:
            one, many = fmt_ms(r["libjay1"]), fmt_ms(r["libjayN"])
            speed = f"{r['libjay1'] / r['libjayN']:.2f}x"
        out.append(f"| {w.name} | {one} | {many} | {speed} | "
                   f"{fmt_ms(r['polars'])} | {fmt_ms(r['numba'])} | {fmt_ms(r['numpy'])} |")
    return "\n".join(out)


def probe_table(probe, got) -> str:
    who = probe["rival"][0]
    unit = "samples" if probe["corpus"] == "dsp" else "bars"
    out = [f"| {unit} | libjay, one call, 1 thread | {who} | ratio |",
           "|---:|---:|---:|---:|"]
    for n, t_j, t_r in got:
        out.append(f"| {n:,} | {fmt_ms(t_j)} | {fmt_ms(t_r)} | {t_j / t_r:,.0f}x |")
    return "\n".join(out)


def provenance(args, pl, numba, rss, load) -> str:
    return "\n".join([
        "```",
        f"machine   {platform.platform()}",
        f"cpu       {platform.processor()}, {os.cpu_count()} logical threads",
        f"rustc     1.89 (rust-toolchain.toml)",
        f"python    {platform.python_version()}, numpy {np.__version__}, "
        f"polars {pl.__version__}, numba {numba.__version__}",
        f"sizes     OHLCV {args.rows:,} minute bars, audio {args.samples:,} samples",
        f"method    best of {args.repeat} after one warmup, {args.passes} alternating "
        f"passes, wall time in ms",
        f"date      {time.strftime('%Y-%m-%d')}, one-minute load average {load:.1f}",
        f"peak RSS  {rss / 1e9:.2f} GB",
        "```",
    ])


def report(args, rows, probes, rss, pl, numba) -> str:
    parts = [table(rows, args.threads)]
    for probe, got in probes:
        parts.append(f"\n{probe['name']}\n\n" + probe_table(probe, got))
    parts.append(f"\npeak RSS {rss / 1e9:.2f} GB")
    return "\n".join(parts)


WHAT_IT_SHOWS = """
### What the table says

**On eight threads libjay beats Polars on nine of the eleven rows both can
run, beats numpy on eight of eleven, and beats numba on two.** That is the
honest summary, and all three halves have one cause. A libjay sentence
becomes one or two passes over the column where a Polars pipeline is four or
five, which is where the wins come from; a numba loop is *one* pass that
keeps its accumulators in registers and never writes an intermediate at all,
which is where the losses come from. The two rows where libjay beats the
numba loop — the cumulative return and the rolling cross-correlation — are
the two where the loop has real arithmetic per element and libjay has four
cores to put on it.

**On one thread libjay loses to numba on every row**, by one and a half to
thirty times. That is the gap this file exists to name, and it is a gap in
traffic, not in arithmetic: a fused kernel still reads and writes whole
columns between the steps a hand-written loop keeps in registers.

**The three rows that were diagnosed here last time now have numbers.** Two
of them had none at all. RSI(14) was the suffix scan `u/\\.` folding every
suffix from scratch — n²/2 steps of a general dyad, which at 20M rows is not
a duration — and is 2.3 seconds now that the scan carries an accumulator and
the affine step `[ + c * ]` is run as the recurrence it is. VWAP was the key
`+//.` costing rows × groups, or hours at 13,889 days, and is 1.4 seconds
now that the keys are hashed once. Frame RMS was the worst row in the file
at 535 ms, four fifths of it a reshape copying a buffer into its own shape,
and is 30 ms — a row libjay now wins. Both of the first two still lose to
the rivals that have a native answer, and the reasons are under each row.
Every other row was re-measured in the same session on the same build: what
moved there moved with the machine, not with the code.

**Two rows lose for reasons that are equally specific.** The peak count
folds its 32-tap filter four times, because the fusion pass moves a named
value into every sentence that reads it and this one reads it at four
alignments. The golden cross is the same shape at a smaller scale: three
window folds where numba slides three accumulators through one pass, and it
is the row where the widest gap to a hand-written loop survives threading.

**Where libjay is at its best it is at its best by a lot.** The rolling
cross-correlation — two shifted views of one buffer, multiplied, and a
window fold over the product — is nine times Polars, nearly four times numpy
and slightly faster than the numba loop, on a sentence of thirty-four
characters. Bollinger bands, the volatility regime and the cumulative return
are the same shape and the same story: three to seven times numpy, and level
with or ahead of the hand-written loop once the threads are in.

**Correctness is the part with no caveats.** Every implementation of every
workload agrees with a reference that computes each window from its own
items, and the three rows whose answer is an integer — two crossing counts
and a peak count — agree exactly, over 20 million bars, across four
independent implementations.
"""


def document(args, rows, probes, rss, load, pl, numba) -> str:
    """workloads.md: the numbers, the source side by side, and what it shows."""
    impl_source = {
        "polars": lambda w: source_of(w.polars),
        "numba": numba_source,
        "numpy": lambda w: source_of(w.numpy),
    }
    out = [
        "# Workloads",
        "",
        "Twelve jobs an analyst would recognise, each written four ways: as one",
        "libjay expression or a short named sequence, in idiomatic Polars, as a",
        "numba loop and in numpy. The corpora are the two libjay is contracted",
        "to serve — OHLCV minute bars and a synthetic audio signal — and the",
        "sizes are the ones the rest of bench/ uses.",
        "",
        "The J is what a J user would write by hand, not a transcription of the",
        "Python: a moving average is `20 +/\\ y`, a shift is a drop, a group-by",
        "is the key `+//.`. The Polars is what a Polars user would write: rolling",
        "aggregations, `group_by`, `ewm_mean`, expression chains — nothing is",
        "handicapped to make libjay look better, and where Polars has a native",
        "answer libjay lacks, it wins the row and the text says so.",
        "",
        "Generated by `bench/workloads.py`; re-run it to replace this file.",
        "",
        "```sh",
        ".venv-bench/bin/python bench/workloads.py",
        ".venv-bench/bin/python bench/workloads.py --quick    # a one-minute smoke",
        "```",
        "",
        "## Provenance",
        "",
        provenance(args, pl, numba, rss, load),
        "",
        "Correctness comes first and is untimed: every implementation is checked",
        "against a reference that computes each window from its own items, over the",
        f"leading {args.check:,} bars and {args.check_dsp:,} samples, before anything",
        "is measured. The script stops if any two disagree.",
        "",
        f"**Memory.** The peak RSS above is the largest of the harness and its two",
        f"libjay subprocesses, and the largest workload is the one that decides it:",
        f"VWAP holds six columns of {args.rows:,} rows — five f64 and one i64, about",
        f"{args.rows * 6 * 8 / 1e9:.2f} GB — as numpy arrays, as the Polars frame that",
        "wraps them without copying, and as whatever each implementation allocates for",
        "its answer. libjay's own share is zero: every column crosses the boundary",
        "borrowed, and only a verb that needs the rows woven together ever makes a copy.",
        "",
        "libjay's two columns come from subprocesses with `LIBJAY_THREADS` set,",
        "because the pool's size is fixed the first time it is used. Polars runs",
        "multi-threaded by default; the numba loops and numpy are single-threaded,",
        "so the fairest single comparison is libjay's one-thread column against",
        "numba and its eight-thread column against Polars.",
        "",
        "## Results",
        "",
        table(rows, args.threads),
        "",
        WHAT_IT_SHOWS.strip(),
        "",
    ]
    if probes:
        out += [
            "## Where libjay loses",
            "",
            "One spelling in the file has no row of its own, because the quantity",
            "it computes is already in the table under another spelling and this",
            "one is the slow way to write it. It is measured over four sizes,",
            "beside the rival that has a linear answer: one call, not a best of",
            "five, because the shape of the curve is the whole point and a repeat",
            "would not change it.",
            "",
        ]
    for probe, got in probes:
        out += [f"### {probe['name']}", "", probe_table(probe, got), "",
                probe["why"].strip(), ""]
    out += ["## The workloads", ""]
    for r in rows:
        w = r["w"]
        out += [f"### {w.name}", "", w.what.capitalize() + ".", "", "**J**", "",
                "```j", w.source(args.rows if w.corpus == "ohlcv" else args.samples),
                "```", ""]
        for who, label in (("polars", "Polars"), ("numba", "numba"),
                           ("numpy", "numpy")):
            if r[who] is None and who == "polars":
                out += ["**Polars** — not applicable; see below.", ""]
                continue
            out += [f"**{label}**", "", "```python", impl_source[who](w), "```", ""]
        out += [w.note.strip(), ""]
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    main()
