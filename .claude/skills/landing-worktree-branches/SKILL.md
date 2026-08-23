---
name: landing-worktree-branches
description: Land a subagent's worktree branch onto main safely — rebase, gate, fast-forward, clean up — with linear history and no merge commits. Use this skill every time a worktree agent reports a finished branch, whenever rebasing or merging agent work, resolving rebase conflicts in shared appended files (CHANGELOG, decision logs), or fast-forwarding main. The two costliest git mistakes in orchestration history happened during exactly this procedure — follow it literally.
---

# Landing worktree branches

## The procedure

1. **Never `cd` into a worktree to land.** Run every git command as `git -C <path>`. A `cd` that lingers across tool calls once made `merge --ff-only` fast-forward the *worktree's own branch onto itself* ("Already up to date") while main never moved — the landing silently didn't happen.
2. Rebase the branch onto main *in its worktree*: `git -C <wt> rebase main`.
3. **Conflicts in append-only files** (CHANGELOG, decisions log): both sides are usually valid appends — keep both, HEAD's first. After any scripted resolution, **verify zero markers before staging**: `grep -c '<<<<<<<' file` must print 0. A resolver script once failed silently (wrong cwd), the conflicted file got staged anyway, and `rebase --continue` committed literal conflict markers. `git add` accepts marker-laden files without complaint — only the grep protects you.
4. Full gate on the rebased tip in the worktree (tests, lints, docs, domain checks, an isolated throwaway venv for Python — deleted afterwards so the branch stays clean). A first-run failure under parallel test threads may be a fresh-build race: re-run once before diagnosing; only a *reproducing* failure is real.
5. Land from the MAIN tree: `git -C <main> merge --ff-only <branch>` — then immediately verify `git -C <main> rev-parse HEAD` equals the branch tip. Push. `--ff-only` guarantees linear history; if it refuses, the rebase is stale — go back to step 2, never merge.
6. Clean up: `git -C <main> worktree remove <wt>`, `branch -d <branch>`, then confirm `worktree list` and `log --merges | wc -l` (must stay 0).
7. Watch CI on the landed commit in the background; treat its verdict, not the local gate, as final.

## Ordering

Land one branch at a time. When several agents share hot files, let the heaviest change land last and give it the final workspace-wide sweep (lints, doc links, stray files) before it reports.

## Launching agents into worktrees

- Prefer the Agent tool's `isolation: "worktree"` — the harness creates the
  worktree and makes it the agent's real cwd. Then the agent needs no git
  setup of its own and EnterWorktree-style tooling works.
- If an agent must create its worktree itself (custom placement under
  .claude/worktrees/), tell it: work through plain shell only — `cd` inside
  each Bash call or `git -C <wt>`/absolute paths. The EnterWorktree TOOL
  cannot switch a session whose cwd is the repository root; agents that
  inherit the root cwd and call it get "Cannot enter worktree: the current
  working directory ... is the repository root". Shell paths are the way.

## Metric-diff gating (not just "checks pass")

A landing compares two measured states, never just "the gate ran". The
incidents that forced this rule: a keep-both conflict resolution once glued
two enums without closing the first — and the gate "passed" because `grep -c
FAILED` counted zero matches *when compilation had failed and no tests ran
at all*; a second resolution silently dropped a function parameter that only
`--workspace` compilation caught.

1. **Baseline first.** Before rebasing, run the suite on main (or use its
   last recorded run) and keep the numbers: tests passed PER BINARY, tests
   skipped/ignored, number of test binaries, corpus expressions replayed.
2. **Collect the same numbers on the rebased tip.** Parse the `test result:`
   lines themselves — never grep for absence of FAILED (absence also means
   "nothing compiled"). Confirm compilation succeeded explicitly before
   counting anything.
3. **Compare, and refuse the landing unless:** zero failures; passed count
   per binary ≥ baseline (a drop means tests vanished — a lost file, a
   module unhooked by the merge); skipped+ignored ≤ baseline; binary count ≥
   baseline; corpus replay count ≥ baseline. "Not worse" is not enough
   stated loosely — each of these is a separate check with a number.
4. **Look at the merge with your eyes.** After any conflict resolution or
   scripted keep-both, read the resolved hunks (`git diff HEAD~1` on the
   resolved files, or the conflicted regions specifically) looking for:
   unbalanced braces/parens, a type or fn signature spliced mid-list,
   duplicated or half-deleted lines, doc comments attached to the wrong
   item. Scripted resolutions handle append-only files; anything inside
   code must be reviewed by reading, then confirmed by `cargo check` on the
   WHOLE workspace (single-crate checks miss dropped parameters in
   dependents).
