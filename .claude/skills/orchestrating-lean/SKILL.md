---
name: orchestrating-lean
description: Keep the main session's token spend minimal by delegating all real work to subagents and reserving the main loop for specs, decisions, landings, and verification summaries. Use this skill whenever running as an orchestrator over subagents on a long multi-step project — especially when the main session runs on a premium model (Fable/Opus) — and whenever about to do implementation, debugging, doc-writing, benchmark-running, or multi-file editing inline instead of delegating it. Trigger it also when deciding how to verify a finished agent's work, when an agent dies mid-task, or when session-limit pressure is visible.
---

# Orchestrating lean

## Why

The main session's context is the scarcest resource: every inline command output, file read, and retry lands in it permanently, and on a premium model it is also the most expensive. Subagents get fresh context, run in parallel, and their transcripts never touch the main window — only their final report does. A main session that implements is paying premium rates to do what a subagent does better in isolation.

## The division

- **Main session (orchestrator):** write task specs, make/record decisions, land branches, answer the user, keep the release/CI state. Nothing else.
- **Opus subagents:** anything touching semantics, engine internals, API migrations, oracle-validated work, structural refactors.
- **Sonnet subagents:** docs sync, examples, CHANGELOG wording, CI/workflow edits, count reconciliation, corpus housekeeping, report-verification passes.
- When in doubt between doing it inline and delegating: a task needing >3 tool calls or any file editing gets delegated.

## Verification without re-doing

Do not re-run an agent's full gate inline — that duplicates its work at premium rates. Instead: one cheap smoke probe of the headline claim (a single CLI invocation, one grep), plus CI as the real arbiter. If deeper verification is warranted, delegate it to a Sonnet checker agent with the specific claims to verify.

## Keeping inline output small

When an inline command is unavoidable, filter at the source: `| grep 'test result'`, `| tail -1`, `awk` summaries — never let a full build log or test run into the context. Background long waits (`run_in_background` + until-loops) so polling costs nothing.

## Interrupted agents

Agents die from session limits, server errors (529), machine sleep, auth expiry. Their worktree/branch state survives. Resume with a message ("take stock: git status, git log, cargo check — then continue your brief"), never relaunch from scratch; a relaunch re-pays the whole ramp-up. If an agent stalls parked on a background notification that never fired, resume it with explicit foreground instructions.
