NB. A Savitzky-Golay filter, designed rather than tabulated.
NB.
NB. The filter that fits a polynomial of degree k by least squares to every
NB. window of 2h+1 samples and reports the fitted value — or the fitted
NB. d-th derivative — at the window's centre. Because the fit is linear in
NB. the samples, the whole operation collapses to one convolution, and the
NB. weights are a row of the pseudo-inverse of the Vandermonde matrix over
NB. the window's offsets: no table of coefficients is needed, and any half
NB. width, degree and derivative order can be asked for (Savitzky and Golay,
NB. Analytical Chemistry 36, 1964).
NB.
NB. Input: `SGCOEF` takes the triple (half width, polynomial degree,
NB. derivative order) and answers 2h+1 weights. `SGFILT` takes those weights
NB. on the left and a signal vector on the right.
NB. Output: `SGFILT` answers (#signal) - 2h values, one per full window, so
NB. it is the interior of the smoothed signal with no invented edges. A
NB. derivative is per sample; divide by the sample spacing to the d-th power
NB. for one per unit of time.
NB.
NB. Checked against bench/algos/savgol.py, which builds the same design
NB. matrix with numpy and takes its pseudo-inverse — a different route to
NB. the same weights — to 1e_10 relative, and against the published
NB. five-point quadratic smoother, which is _3 12 17 12 _3 over 35.

NB. The weights. `i: h` is the window's offsets and `(i: h) ^/ i. >: k` the
NB. Vandermonde matrix over them; the pseudo-inverse of that matrix is
NB. `(inverse of A transpose-times-A) times A transpose`, whose d-th row
NB. already holds the d-th Taylor coefficient — so the derivative wants only
NB. the factorial.
SGCOEF =. 3 : 0
  h =. 0 { y
  k =. 1 { y
  d =. 2 { y
  a =. (i: h) ^/ i. >: k
  g =. (%. (|: a) +/ . * a) +/ . * |: a
  (! d) * d { g
)

NB. The convolution. `n u\ y` applies `u` to every window of n items, so the
NB. filter is the weighted sum of each window — one sentence, and no state.
SGFILT =. 4 : '(# x) (+/ @ (x & *))\ y'

NB. === demo ==================================================================

echo 'the classic quadratic five-point smoother, times 35:'
echo 35 * SGCOEF 2 2 0

echo ''
echo 'and its first derivative (h=2, k=2, d=1), times 10:'
echo 10 * SGCOEF 2 2 1

NB. A noisy sine, smoothed and differentiated.
NB.
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

n =. 64
t =. (i. n) % n
sig =. (1 o. 6 * t) + 0.1 * 13 RANDOM n

echo ''
echo 'noise left in the signal, and left after a 9-point cubic smoother:'
echo %: (+/ *: sig - 1 o. 6 * t) % n
sm =. (SGCOEF 4 3 0) SGFILT sig
echo %: (+/ *: sm - (4 }. _4 }. 1 o. 6 * t)) % # sm

NB. The weights estimate the derivative per SAMPLE; the signal is sampled n
NB. times over the unit interval, so multiplying by n turns it into a
NB. derivative per unit of t.
echo ''
echo 'the derivative it estimates, and 6 * cos 6t, at five points:'
echo 5 {. n * (SGCOEF 8 3 1) SGFILT sig
echo 5 {. 8 }. 6 * 2 o. 6 * t
