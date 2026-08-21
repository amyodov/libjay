# Examples

Every example is runnable as-is — no APL keyboard needed, the glyphs are
already in the files.

From a checkout (after `maturin develop`, or via `uvx --from . libjay`):

```sh
libjay hello.ijs              # J:   Hello, world!
libjay hello.apl              # APL: Hello, world!
libjay stats.ijs              # mean and range of a series
libjay reduce_axis.ijs        # J:   +/ sums along the LEADING axis
libjay reduce_axis.apl        # APL: +/ trailing axis, +⌿ leading
libjay timeseries.ijs         # a moving-window Bollinger z-score
libjay boxes.ijs              # a boxed list of strings, opened and razed
libjay defs.ijs               # an explicit verb, recursive, with if.
libjay dfn.apl                # a dfn, with the ⍺← default-argument idiom
python quickstart.py          # the Python API end to end
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
