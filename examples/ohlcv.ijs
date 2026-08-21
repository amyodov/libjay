NB. Three indicators over twelve minute bars.
NB.
NB. The sentences are the ones bench/workloads.py runs over 20 million rows;
NB. only the window lengths are smaller, so the whole answer fits on a page.

close =. 100 101 99 102 104 103 105 107 106 108 109 107

echo 'closes:'
echo close

NB. 1. Simple returns, in per cent. `1 }. close` is every bar but the first
NB. and `_1 }. close` every bar but the last, so dividing one by the other
NB. pairs each bar with the one before it.
echo ''
echo 'returns (%):'
echo 100 * _1 + (1 }. close) % _1 }. close

NB. 2. The 3-bar moving average. `3 +/\ close` sums every window of three
NB. bars; `2 }. close` drops the two bars no window covers, which is what
NB. lines the closes up with their own averages.
ma =. (3 +/\ close) % 3
echo ''
echo '3-bar moving average:'
echo ma
echo 'close above its average?'
echo (2 }. close) > ma

NB. 3. Maximum drawdown: `>./\ close` is the running peak, so `close % >./\
NB. close` is how far each bar is below its own high-water mark, and `>./`
NB. takes the worst of them.
echo ''
echo 'maximum drawdown (%):'
echo 100 * >./ 1 - close % >./\ close
