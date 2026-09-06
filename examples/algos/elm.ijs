NB. An extreme learning machine.
NB.
NB. A one-hidden-layer network whose hidden layer is never trained. The
NB. input weights and biases are drawn once and left alone; the hidden
NB. activations they produce are treated as features, and the only thing
NB. fitted is the linear readout on top of them — which makes training a
NB. single ridge-regularised least-squares solve rather than an epoch loop.
NB. With enough hidden units it approximates any continuous function, and it
NB. trains in one pass (Huang, Zhu and Siew, Neurocomputing 70, 2006).
NB.
NB. Input: `y` is the boxed pair (inputs ; targets), the inputs an n-by-d
NB. matrix and the targets n-by-c; `x` is the boxed triple (input weights
NB. d-by-h ; biases h ; ridge penalty).
NB. Output: the readout, an h-by-c matrix. `PREDICT` applies it.
NB.
NB. Checked against bench/algos/elm.py, which builds the same normal
NB. equations with numpy and solves them with numpy.linalg.solve, to 1e_8
NB. relative on the fitted values — the tolerance a 40-by-40 ridge solve
NB. earns, and no tighter.

NB. The hidden layer: one matrix product, the bias added along each row,
NB. through tanh — which is `7 o.` in J.
HIDDEN =. 4 : 0
  w =. > 0 { x
  b =. > 1 { x
  7 o. (y +/ . * w) +"1 b
)

NB. The readout by the normal equations. Forming H'H costs one pass over
NB. the n-by-h features and leaves an h-by-h system, which is the whole
NB. reason the machine is fast: n never enters the solve. `=/~ i. h` is the
NB. identity matrix, and `%.` is the matrix divide, so `b %. a` solves
NB. a mp z = b.
RIDGE =. 4 : 0
  h   =. > 0 { y
  t   =. > 1 { y
  ht  =. |: h
  (ht +/ . * t) %. ((ht +/ . * h) + x * =/~ i. {: $ h)
)

NB. Training, end to end: the hidden layer once, then the one solve.
ELMFIT =. 4 : 0
  hid =. (2 {. x) HIDDEN > 0 { y
  (> 2 { x) RIDGE hid ; > 1 { y
)

NB. `x` is the boxed pair (input weights ; biases) followed by the readout;
NB. `y` the inputs.
PREDICT =. 4 : '((2 {. x) HIDDEN y) +/ . * > 2 { x'

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

NB. Learn a one-dimensional function from 400 samples with 40 hidden units.
n =. 400
h =. 40
xs =. (i. n) % <: n
tgt =. ,. (1 o. 6 * xs) + 0.05 * 3 RANDOM n     NB. a sine plus noise
inp =. ,. xs
w =. (1, h) $ 8 * 5 RANDOM h
b =. 4 * 9 RANDOM h

net =. w ; b ; 1e_4
beta =. net ELMFIT inp ; tgt

echo 'readout shape (hidden units by outputs):'
echo $ beta

fit =. (w ; b ; beta) PREDICT inp
echo ''
echo 'root mean squared error against the noiseless sine:'
echo %: (+/ *: (, fit) - 1 o. 6 * xs) % n

echo ''
echo 'the fit at eleven evenly spaced points:'
echo , (w ; b ; beta) PREDICT ,. (i. 11) % 10
