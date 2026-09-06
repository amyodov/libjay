NB. The Hurst exponent by rescaled range.
NB.
NB. Hurst's R/S statistic asks how far a series wanders from its own mean
NB. relative to how much it wiggles. Cut the series into blocks of a given
NB. length, and within each block take the running sum of the deviations
NB. from the block mean; R is the range of that running sum and S the
NB. block's standard deviation, taken over the block itself so the divisor
NB. is the block length. R/S grows like (block length)^H, so the
NB. slope of log(R/S) against log(length) is the exponent: 0.5 for a random
NB. walk's increments, above 0.5 for a persistent series and below for a
NB. mean-reverting one.
NB.
NB. Input: `y` is the series (increments, not levels — feed it returns);
NB. `x` is the vector of block lengths to measure, each at least 2 and no
NB. longer than the series.
NB. Output: two numbers — the exponent and the intercept of the fit, so
NB. that R/S at length L is (^ intercept) * L ^ exponent.
NB.
NB. Checked against bench/algos/hurst.py, written from Hurst's definition
NB. as Mandelbrot and Wallis state it, to 1e_11 relative.

NB. The mean rescaled range at one block length. `x` is the length, `y` the
NB. series; the tail that does not fill a whole block is dropped. Every
NB. block is a row of one matrix, so the mean, the running sum, the range
NB. and the deviation are each one rank-1 verb applied across it.
RS =. 4 : 0
  k   =. <. (# y) % x
  b   =. (k, x) $ (k * x) {. y
  dev =. b -"1 0 (+/"1 b) % x
  cs  =. +/\"1 dev
  r   =. (>./"1 cs) - <./"1 cs
  s   =. %: (+/"1 *: dev) % x
  (+/ r % s) % k
)

NB. The exponent: an ordinary least-squares slope through the log-log
NB. points. A fit needs both centred vectors, and the intercept follows from
NB. the two means.
HURST =. 4 : 0
  lx =. ^. x
  ly =. ^. x RS"0 _ y
  mx =. (+/ lx) % # lx
  my =. (+/ ly) % # ly
  dx =. lx - mx
  h  =. (+/ dx * ly - my) % +/ *: dx
  h , my - h * mx
)

NB. === demo ==================================================================

NB. A deterministic counter-based generator, so the demo is the same number
NB. in every implementation: a counter multiplied into a Mersenne prime's
NB. residues and squared twice — the squaring is what stops it being a
NB. lattice, since a chain of multiplications modulo a prime is just one
NB. multiplication and consecutive counters would come out a fixed distance
NB. apart. Every product is kept under 2^63, so the arithmetic is exact.
RANDOM =. 4 : 0
  p =. 2147483647
  u =. p | 1103515245 * x + >: i. y
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 48271 * u
  _1 + 2 * u % p
)

n =. 8192
w =. 11 RANDOM n
lens =. 16 32 64 128 256 512

echo 'block lengths:'
echo lens

echo ''
echo 'R/S of white noise, and its exponent and intercept:'
echo lens RS"0 _ w
echo lens HURST w

NB. A persistent series: the same noise averaged over a 64-sample window,
NB. which correlates every value with its 63 neighbours. Blocks shorter than
NB. the window see a smooth trend and blocks much longer see the noise
NB. again, so the exponent lands between the two.
mem =. (64 +/\ w) % 64
echo ''
echo 'R/S of a 64-sample memory of that noise, and its exponent:'
echo lens RS"0 _ mem
echo lens HURST mem
