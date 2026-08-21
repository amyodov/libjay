⍝ A fork (f g h): f and h apply, g combines — sum ÷ count is the mean.
⎕←(+/÷≢) 3 1 4 1 5 9 2 6
⍝ An atop (g h): h applies first, then g to what it returns.
⎕←(-⌈/) 3 1 4 1 5 9 2 6
⍝ A noun may stand as a fork's left tine, not just a function.
⎕←(10-÷) 4
⍝ A tacit function can be named, then applied like any other verb.
M←+/÷≢
M 3 1 4 1 5 9 2 6
