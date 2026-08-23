<!-- title: v0.3.1 — Dyalog dialect: structure, control words, shy results -->
# libjay 0.3.1

The Dyalog dialect that headlined 0.3.0 closes most of its named gaps: values now carry Dyalog's box structure through the inner product, the four missing control words work, and a dfn whose result is an assignment answers shyly. Agreement with the recorded Dyalog 20 answers rises from 96.5% to 98% — 1,984 of 2,025, with every remaining row itemised by cause.

## What changed

**The inner product nests as Dyalog nests.** Under `Dialect::dyalog()`, `f.g` on nested operands folds pairs of elements the way Dyalog does, one enclosure shallower than the APL2 reading — so probing a Game of Life generation with `≡` now answers 1, exactly as a real Dyalog session does, and dissecting the pipeline with `⊃` picks the same parts. The APL2/GNU reading — still the default — is untouched, byte for byte.

**Control flow is complete.** `:AndIf` and `:OrIf` short-circuit their `:If`/`:While` chains, `:Select` arms take `:CaseList`, and `:For a b :In` destructures. A function may be called before the `∇`/`⎕FX` line that defines it, as Dyalog allows.

**Shy results.** `F←{r←⍵×2} ⋄ F 5` displays nothing — the value flows when consumed (`1+F 5` is 11) but a session line ending in an assignment-resulting dfn stays quiet. The rule came from the oracle, not the manual: shyness survives `¨` and `⍣` but not `⊢` or `⌽`, because it belongs to the application, not the sentence.

## Also

- Corpus: 7,900+ expressions, 9,900+ recorded oracle answers across jconsole, GNU APL, and Dyalog 20 — all three replayed as hard gates on every commit; the Dyalog preset's exemption ledger shrank from 71 rows to 41 (23 deliberate divergences, 18 open gaps, the largest now `⎕R`/`⎕S`).
- Fixes that rode along since 0.3.0: a panic on a boxed huge cycle index in J's `C.`, empty-argument checks that fired where the references answer, outfix's eager type probe, and catenate's empty-type rule.

## Install

```sh
uvx libjay --lang apl --dialect dyalog -e '≡{↑1 ⍵∨.∧3 4=+/,¯1 0 1∘.⊖¯1 0 1∘.⌽⊂⍵} 4 6⍴0 1'   # 1, as Dyalog says
uv add libjay    # or: pip install libjay / cargo add libjay
```

## Links

- [Changelog](../CHANGELOG.md) · [Status matrix](status.md) · [Which APL](coverage.md) · [Examples](../examples/)
