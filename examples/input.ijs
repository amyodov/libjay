NB. 1!:1 ]1 reads one line of input as a character vector.
line =. 1!:1 ]1
NB. x 1!:2 ]2 writes x's characters and nothing else — no newline added.
NB. Assignments yield nothing, so this is the only output on the line.
w =. ('you said: ',line) 1!:2 ]2
