# Examples

Every example is runnable as-is — no APL keyboard needed, the glyphs are
already in the files.

No install needed — `uvx libjay` fetches the prebuilt wheel; from a checkout,
`uv run libjay` (after `maturin develop`) works the same way:

```sh
uvx libjay hello.ijs              # J:   Hello, world!
uvx libjay hello.apl              # APL: Hello, world!
uvx libjay stats.ijs              # mean and range of a series
uvx libjay reduce_axis.ijs        # J:   +/ sums along the LEADING axis
uvx libjay reduce_axis.apl        # APL: +/ trailing axis, +⌿ leading
uvx libjay timeseries.ijs         # a moving-window Bollinger z-score
uvx libjay ohlcv.ijs              # returns, a moving average and drawdown
uvx libjay boxes.ijs              # a boxed list of strings, opened and razed
uvx libjay defs.ijs               # an explicit verb, recursive, with if.
uvx libjay dfn.apl                # a dfn, with the ⍺← default-argument idiom
uvx libjay trains.apl             # a fork, an atop, a noun tine, a named tacit fn
uvx libjay modifiers.ijs          # explicit adverb/conjunction, {{ }}, a named adverb
uvx libjay exact.ijs              # extended integers and exact rationals
uvx libjay complex.ijs            # sqrt of a negative, polar form
printf 'hello\n' | uvx libjay examples/input.apl   # ⍞ reads a line, ⍞← echoes it
printf 'hello\n' | uvx libjay examples/input.ijs   # 1!:1 ]1 / 1!:2 ]2, same idea
uv run --with libjay --no-project python quickstart.py          # the Python API end to end
```

## Output

### `hello.ijs`

```
Hello, world!
```

### `hello.apl`

```
Hello, world!
```

### `stats.ijs`

```
3.875
8
```

### `reduce_axis.ijs`

```
0 1 2
3 4 5
3 5 7
3 12
```

### `reduce_axis.apl`

```
1 2 3
4 5 6
6 15
5 7 9
```

### `timeseries.ijs`

The same moving-sum shape as the phase-5 benchmark kernel
([bench/README.md](../bench/README.md#phase-5-one-expression-against-a-rolling-pipeline)),
run here on twelve closes and a window of 3 instead of 20M rows and a
window of 20:

```
closes:
100 101 99 102 104 103 105 107 106 108 109 107
z-score of each window:
_1.22474 1.06904 1.13555 0 1.22474 1.22474 0 1.22474 1.06904 _1.22474
```

### `ohlcv.ijs`

Three of the indicators [bench/workloads.md](../bench/workloads.md) measures
over 20 million minute bars, run here on twelve hand-written closes: the
sentences are the same, only the windows are shorter.

```
closes:
100 101 99 102 104 103 105 107 106 108 109 107

returns (%):
1 _1.9802 3.0303 1.96078 _0.961538 1.94175 1.90476 _0.934579 1.88679 0.925926 _1.83486

3-bar moving average:
100 100.667 101.667 103 104 105 106 107 107.667 108
close above its average?
0 1 1 0 1 1 0 1 1 0

maximum drawdown (%):
1.9802
```

### `boxes.ijs`

```
boxed names:
+--+---+-+
|ab|cde|f|
+--+---+-+
length of each, opened by &.>, so the answer comes back boxed too:
+-+-+-+
|2|3|1|
+-+-+-+
joined back together (raze):
abcdef
```

### `defs.ijs`

```
fac 5:
120
720
```

### `dfn.apl`

```
max, mean, min:
9 3.875 1
no left argument, the default 1:
3 1 4
left argument 10:
30 10 40
```

### `trains.apl`

```
3.875
¯9
9.75
3.875
```

### `modifiers.ijs`

```
16
4
18
10
```

### `exact.ijs`

```
265252859812191058636308480000000
5r6
15511210043330985984000000
```

### `complex.ijs`

```
0j2
2 1.5708
0.707107j0.707107
```

### `input.apl` / `input.ijs`

Run as `printf 'hello\n' | uvx libjay examples/input.apl` (piped input, since
`⍞`/`1!:1 ]1` read a line from standard input); both print the same line:

```
you said: hello
```
