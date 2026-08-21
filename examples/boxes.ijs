NB. Boxes: J's container for values that don't share one shape — here, a
NB. list of strings. `<` boxes an item, `;` links items into a boxed list
NB. (and razes them open again), `&.>` runs a verb on each box's contents
NB. and reboxes the result.
names =. 'ab';'cde';'f'
echo 'boxed names:'
echo names
echo 'length of each, opened by &.>, so the answer comes back boxed too:'
echo # &.> names
echo 'joined back together (raze):'
; names
