NB. The Lo-MacKinlay variance-ratio test.
NB.
NB. If a log price is a random walk its variance grows linearly with the
NB. horizon, so the variance of a q-bar return divided by q times the
NB. variance of a one-bar return is 1. The statistic is that ratio, computed
NB. with overlapping q-bar returns and the unbiased corrections of Lo and
NB. MacKinlay (Review of Financial Studies 1, 1988), together with the two
NB. standardised statistics they give: z1 assumes homoskedastic increments,
NB. z2 is robust to heteroskedasticity and is the one to read on real bars.
NB. Above 1 the series trends (positive autocorrelation), below 1 it
NB. mean-reverts; |z| above about 1.96 rejects the walk at 5%.
NB.
NB. Input: `y` is a vector of LOG prices, n+1 of them; `x` is the horizon q,
NB. an integer at least 2 and less than n.
NB. Output: three numbers — the ratio, z1, z2.
NB.
NB. Checked against bench/algos/varratio.py, written from the paper's
NB. equations, to 1e_11 relative.

NB. The j-th autocovariance of the SQUARED demeaned returns, unnormalised —
NB. `VR` scales it. `x` is the lag, `y` the squared demeaned returns.
DELTA =. 4 : '+/ (x }. y) * (-x) }. y'

VR =. 4 : 0
  q  =. x
  n  =. <: # y
  mu =. (({: y) - {. y) % n
  e  =. mu -~ 2 -~/\ y            NB. demeaned one-bar returns
  s2 =. *: e
  va =. (+/ s2) % <: n            NB. one-bar variance, unbiased
  ec =. (q * mu) -~ (q }. y) - (-q) }. y
  NB. J reads right to left, so the count of overlapping windows is written
  NB. `1 + n - q` here: `n - q + 1` would be n minus q plus one.
  m  =. q * (1 + n - q) * 1 - q % n
  vc =. (+/ *: ec) % m            NB. q-bar variance, overlapping, unbiased
  vr =. vc % va
  z1 =. ((%: n) * vr - 1) % %: (2 * (<: 2 * q) * <: q) % 3 * q
  js =. >: i. <: q
  dl =. (n * js DELTA"0 _ s2) % *: +/ s2
  th =. +/ dl * *: 2 * (q - js) % q
  z2 =. ((%: n) * vr - 1) % %: th
  vr , z1 , z2
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

NB. A pure random walk, and a walk with a trend-following drift added: the
NB. second bar of each pair repeats a fraction of the first bar's move, so
NB. the increments correlate positively and the ratio should sit above 1.
n =. 4000
w =. 7 RANDOM n
walk =. +/\ w * 0.01
push =. +/\ (w + 0.6 * _1 |. w) * 0.01

echo 'q, ratio, z1, z2 for a random walk:'
echo (2 3 4 5 ,. (2 3 4 5) VR"0 _ walk)

echo ''
echo 'the same walk with a one-bar momentum added:'
echo (2 3 4 5 ,. (2 3 4 5) VR"0 _ push)
