⍝ A dfn: ⍵ is the right argument, ⍺ the left, ⋄ separates statements, and
⍝ a dfn's value is its last statement — nothing else is displayed.
Stats ← {m←(+/⍵)÷≢⍵ ⋄ (⌈/⍵),m,(⌊/⍵)}
⎕←'max, mean, min:'
⎕←Stats 3 1 4 1 5 9 2 6

⍝ ⍺←v gives the left argument a value only where none arrived — a default,
⍝ not an unconditional assignment.
Scale ← {⍺←1 ⋄ ⍺×⍵}
⎕←'no left argument, the default 1:'
⎕←Scale 3 1 4
⎕←'left argument 10:'
10 Scale 3 1 4
