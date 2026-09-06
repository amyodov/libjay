NB. Kaufman's Adaptive Moving Average.
NB.
NB. An exponential average whose smoothing constant is chosen bar by bar
NB. from how efficiently the price moved. The efficiency ratio is the net
NB. displacement over a lookback divided by the path length actually walked
NB. in getting there: 1 for a straight line, near 0 for a series that went
NB. nowhere noisily. The ratio is mapped between a fast and a slow
NB. exponential constant and squared, and the average follows
NB.
NB.   k[i] = k[i-1] + sc[i] * (p[i] - k[i-1])
NB.
NB. Input: `y` is a numeric vector of closes, `x` the three periods
NB. (efficiency-ratio lookback, fast, slow) — Kaufman's own 10 2 30.
NB. Output: a vector (#y) - lookback long, the first item seeded with the
NB. close at the lookback bar.
NB.
NB. Checked against bench/algos/kama.py, which is written from Kaufman's
NB. description (New Trading Systems and Methods) rather than from this
NB. code, to 1e_12 relative.

NB. The efficiency ratio over a lookback of `x` bars. `2 -~/\ y` is the
NB. bar-to-bar move, so `x +/\` of its magnitude is the path length; the
NB. net move is the same vector minus itself shifted by the lookback. A
NB. window that never moved has ratio 0 rather than a division by zero.
ER =. 4 : 0
  path =. x +/\ | 2 -~/\ y
  net  =. | (x }. y) - (-x) }. y
  net % path + path = 0
)

NB. One step of the recursion. Fold hands the item on the left and the
NB. running value on the right, so `x` is a (smoothing constant, price)
NB. pair and `y` is the average so far.
KSTEP =. 4 : 'y + ({. x) * ({: x) - y'

NB. The average itself: the ratio scaled between the two constants, squared,
NB. and then folded through the recursion from a seed of the first close the
NB. ratio exists for.
KAMA =. 4 : 0
  per  =. 0 { x
  fast =. 2 % 1 + 1 { x
  slow =. 2 % 1 + 2 { x
  sc   =. *: slow + (per ER y) * fast - slow
  q    =. per }. y
  seed =. {. q
  seed , seed (] F:. KSTEP) }. sc ,. q
)

NB. === demo ==================================================================

close =. 100 101 99 102 104 103 105 107 106 108 109 107 111 110 114 113 118 117

echo 'closes:'
echo close

echo ''
echo 'efficiency ratio over 4 bars:'
echo 4 ER close

echo ''
echo 'KAMA, 4 2 30:'
echo 4 2 30 KAMA close

NB. A trending stretch drives the ratio towards 1 and the average towards
NB. the price; a choppy one parks it near the slow constant and the average
NB. barely moves.
echo ''
echo 'KAMA lags the close by:'
echo (4 }. close) - 4 2 30 KAMA close
