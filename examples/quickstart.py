"""The Python API end to end. Run: python quickstart.py"""

import jay
from jay import apl, j

# One-shot: compile + bind + execute.
print(j("+/ 1 2 3 4"))                     # 10
print(j("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]}))   # 2.8 — the mean

# The same data, both languages, diverging where the languages diverge.
print(j("+/ i. 2 3").tolist())             # [3, 5, 7]  — leading axis
print(apl("+/2 3⍴⍳6").tolist())            # [6, 15]    — trailing axis
print(apl("+⌿2 3⍴⍳6").tolist())            # [5, 7, 9]  — leading again

# Compile once, bind data, run many times.
k = jay.j.compile("+/ {weights} * {data}")
k1 = k.bind({"weights": [0.5, 0.25, 0.25]})
print(k1({"data": [10.0, 20.0, 30.0]}))    # 17.5
print(k1({"data": [4.0, 8.0, 8.0]}))       # 6.0

# Sequences read like a session: intermediate names, last value returned.
print(j("x =. 1 2 3 4\n(>./ x) - <./ x"))  # 3 — the range

# Naming a verb: the name is a verb everywhere after it.
print(j("mean =. +/ % #\nmean 3 1 4 1 5"))  # 2.8

# See what the expression became: the fork, the fused kernel, the shapes.
print(k1.explain({"data": [10.0, 20.0, 30.0]}))

# Errors point into the expression.
try:
    j("1 2 + 1 2 3")
except jay.JayError as e:
    print(e)
