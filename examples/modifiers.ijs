NB. 1 : 'u u y' is an adverb: derives "apply u twice" from any verb u.
twice =. 1 : 'u u y'
echo *: twice 2
NB. 2 : 'u v y' is a conjunction: composes the right verb into the left.
compose =. 2 : 'u v y'
echo *: compose -: 4
NB. {{ }} reads its part of speech from the operand name its body uses.
dbl =. {{u+u}}
echo *: dbl 3
NB. m =. / names the built-in adverb; + m 1 2 3 4 is the familiar +/.
m =. /
+ m 1 2 3 4
