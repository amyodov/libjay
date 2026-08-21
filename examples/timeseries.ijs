NB. Bollinger z-score, window 3 — the shape of the phase-5 benchmark kernel
NB. (bench/README.md), run here on twelve closes instead of 20M rows.
NB. s is the moving sum; the window's mean and variance both come out of it
NB. without a second pass over the window (mean = s % w, and the sum of
NB. squares minus s*s gives the variance up to a factor of w).
close =. 100 101 99 102 104 103 105 107 106 108 109 107
w =. 3
s =. w +/\ close
echo 'closes:'
echo close
echo 'z-score of each window:'
((w * (w - 1) }. close) - s) % %: (w * w +/\ *: close) - s * s
