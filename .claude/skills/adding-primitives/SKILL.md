---
name: adding-primitives
description: The full workflow for implementing J or APL primitives, syntax features, or semantic changes in libjay — oracle probing, implementation, corpus recording, and the documentation set. Use this skill for any coverage wave, any task that adds or changes what a J/APL spelling means, any fix to a primitive's semantics, and any task whose brief mentions docs/status.md rows, the oracles, or the corpus.
---

# Adding primitives

## The invariant

The reference interpreter is the specification. Where a published description, a guess, or this brief disagrees with the oracle, THE ORACLE WINS — implement what the oracle answers and report the correction.

Oracles (run-only, never read their source, never quote their docs):
- J: `~/projects/libjay-oracles/j/j64/jconsole -jprofile /dev/null` (pipe one sentence to stdin)
- GNU APL: `~/projects/libjay-oracles/gnu-apl/install/bin/apl --script --safe --eval '<expr>'`

## The sequence

1. **Probe before coding.** For every meaning to implement, run the oracle on the edge cases first: empties, rank 0 vs one-element vectors, negative/zero parameters, mixed dtypes, boxes, the exact display. Derive the rule from answers, not from memory. A feature GNU APL lacks entirely (Dyalog-era) is implemented from published definitions, marked "no oracle" in status.md, unit-tested by hand, and kept OUT of the corpus.
2. **Implement** in the engine (`verb.rs` ops + frontend table entries), routing any APL2-vs-Dyalog divergence through the `Dialect` object, never hard-wired.
3. **Tests**: end-to-end cases in a wave test file (compile → run → expected values, both languages where both have the spelling).
4. **Corpus**: add expressions to `crates/libjay/tests/corpus/<lang>/<theme>.txt`, record with `cargo run -p libjay-devtools -- record <lang> <file>`, review the snapshot diff, and confirm `record <lang> --check` exits 0. Dialect disagreements go to `corpus/apl/divergences.txt` with a `?` note — pinned to STAY divergent.
5. **Docs, all four**: `docs/status.md` (rows + the summary counts — recount, don't increment), `docs/coverage.md` (tables + gap list), `docs/decisions.md` (dated entry for any design choice or oracle correction), `CHANGELOG.md` Unreleased (user-facing wording, syntax users type, no internal names).
6. **Gate**: `cargo test -p libjay -p libjay-capi -- --test-threads=2`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS=-D warnings cargo doc -p libjay --no-deps`, `cargo deny check`, `record j --check` and `record apl --check`, pytest via a throwaway venv built from the working tree (delete it and `python/jay/_jay.abi3.so` afterwards).

## Deferral is honest

A meaning too deep for the wave stays a named gap: the compiler must answer "not supported yet: <name>" — never a bare syntax error, never a wrong answer. Report deferred items with reasons.
