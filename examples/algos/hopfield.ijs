NB. A Hopfield network: Hebbian storage and synchronous recall.
NB.
NB. A fully connected network of two-state units whose weights are the sum
NB. of the outer products of the patterns to be remembered, with the
NB. self-connections removed. Recall drives a corrupted probe towards the
NB. sign of its own local field, one synchronous sweep at a time; because
NB. the weights are symmetric the sweeps descend the energy
NB.
NB.   E(s) = _0.5 * s W s
NB.
NB. and settle into a stored pattern — as long as the load stays under
NB. Hopfield's 0.138 patterns per unit (PNAS 79, 1982; Amit, Gutfreund and
NB. Sompolinsky for the capacity).
NB.
NB. Input: `STORE` takes a p-by-n matrix of patterns whose entries are _1
NB. and 1. `RECALL` takes the boxed pair (weights ; sweeps) on the left and
NB. a matrix of probes, one per row, on the right — a whole batch of probes
NB. is one matrix product per sweep.
NB. Output: the recalled states, the same shape as the probes.
NB.
NB. Checked against bench/algos/hopfield.py, written from the same three
NB. equations with numpy, exactly — every value is _1 or 1, so the states
NB. must match cell for cell and the energies to 1e_12 relative.

NB. Hebbian storage. The outer product of the pattern matrix with itself
NB. sums over patterns already; `-. =/~ i. n` is the identity matrix
NB. complemented, which is what clears the diagonal.
STORE =. 3 : 0
  n =. {: $ y
  (((|: y) +/ . * y) * -. =/~ i. n) % n
)

NB. One synchronous sweep: every unit takes the sign of its field at once.
NB. `x` is the weight matrix and `y` a matrix of states, one per row, so a
NB. whole batch sweeps in one matrix product. A unit whose field is exactly
NB. zero keeps the state it had, which is the convention that makes a stored
NB. pattern a fixed point.
FLIP =. 4 : 0
  f =. y +/ . * x
  (* f) + y * 0 = f
)

RECALL =. 4 : '((> 0 { x) & FLIP) ^: (> 1 { x) y'

NB. The energy of each state in a batch.
ENERGY =. 4 : '_0.5 * +/"1 y * y +/ . * x'

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

NB. Three patterns over 64 units, and probes made by flipping 12 of the 64
NB. bits of each. The sign of a symmetric random vector is a fair coin.
n =. 64
pat =. * (3, n) $ 17 RANDOM 3 * n
w =. STORE pat

NB. The bits to corrupt: the 12 largest of a fresh random row per pattern.
noise =. (3, n) $ 23 RANDOM 3 * n
mask =. _1 ^ (12 > /:"1 /:"1 noise)
probe =. pat * mask

echo 'bits wrong in each probe:'
echo +/"1 probe ~: pat

recalled =. (w ; 6) RECALL probe
echo ''
echo 'bits wrong after six sweeps:'
echo +/"1 recalled ~: pat

echo ''
echo 'energy of the probes, and of what they settled into:'
echo w ENERGY probe
echo w ENERGY recalled
