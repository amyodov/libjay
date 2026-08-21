NB. An explicit definition (3 : 0 ... ) is a verb like any other, written
NB. over several lines; if. do. else. end. is a control word, legal only
NB. inside one.
fac =. 3 : 0
if. y <: 1 do.
  1
else.
  y * fac y - 1
end.
)
echo 'fac 5:'
echo fac 5
NB. the value of the last sentence is the value of the program
fac 6
