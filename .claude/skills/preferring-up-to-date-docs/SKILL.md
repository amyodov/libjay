---
name: preferring-up-to-date-docs
description: Consult current library documentation (via Context7 or the official docs) before working against any third-party library, framework, tool, or service — and use it to discover better alternatives already built into a dependency. Use this skill whenever writing or editing code that calls an external library's API (pyo3, wgpu, arrow, rayon, polars, numpy, maturin, pytest, serde, tokio, any crate or package), migrating a dependency to a new major/minor version, fixing a compile or runtime error that names a library symbol, writing configuration for a third-party tool (cargo-deny, GitHub Actions, dependabot, maturin, pyproject build backends), choosing between libraries or approaches, or about to hand-write a helper that a dependency might already provide. Trigger even when the task looks familiar — training knowledge of fast-moving libraries is stale by default.
---

# Current library docs first

## Why this exists

Two failure modes cost real time and quality, and both are invisible while they happen:

1. **API drift.** Libraries move faster than training data. Working from memory against pyo3, wgpu, or polars means discovering the current API one compile error at a time — each error teaches one symbol, while the migration guide would have taught all of them at once. A version-bump task done from compile errors alone also misses *behavioral* changes that still compile.
2. **Missed built-ins.** Dependencies grow helpers. Hand-writing a retry loop, a capsule wrapper, a matrix routine, or a config dance that the library already ships is wasted code that must then be maintained. The check costs one query; the miss costs a review cycle later.

## When to query

Query Context7 **before writing the code**, not after it fails:

- **New use of a library API** — about to call into a dependency you haven't touched this session, or use a feature of it you haven't used before.
- **Version migration** — any dependency bump beyond a patch. Query for the migration guide / changelog topics first; fix from the guide, not from the error stream.
- **Errors naming a library symbol** — a compile error, deprecation warning, or runtime exception that mentions a dependency's type or function. One query usually explains the whole family of errors.
- **Third-party configuration** — config files whose schema belongs to someone else (deny.toml, CI actions, build backends). Their formats change; memory of them is the least reliable kind.
- **"Does X already do this?"** — before hand-writing anything generic (pooling, hashing, serialization, FFI plumbing), one query to check the dependency's own surface.
- **Choosing a dependency or approach** — compare current capabilities, not remembered ones.

## When not to query

- The language standard library and language syntax — stable, well-known.
- The project's own code — read the code.
- Anything covered by a project-specific source of truth that outranks documentation (e.g. a reference implementation used as an oracle) — follow the project's rule.
- Mid-flow re-queries for an API already confirmed this session.

## How

1. `resolve-library-id` with the library's common name; pick the match with the right ecosystem (crate vs npm vs pip — check the description, not just the name).
2. `query-docs` with the resolved id and a **specific question** ("breaking changes in 0.29 module init", "buffer mapping API", "deny.toml bans syntax") — not a bare library name. Narrow topics return the relevant page; broad ones return the README you already know.
3. If Context7 lacks the library or returns stale content, fall back to the official docs via web fetch — the point is current documentation, not the specific tool.

## Examples

**Version migration:**
Task: bump pyo3 0.25 → 0.29 in a binding crate.
Do: resolve `pyo3` → query "migration guide 0.29 breaking changes" → apply the guide's rename list (`PyObject` → `Py<PyAny>`, `downcast` → `cast`, capsule API) in one pass.
Not: bump the version, run cargo check, fix 40 errors one by one, and miss the behavioral change that still compiles.

**Config schema:**
Task: write deny.toml banning specific crate versions.
Do: resolve `cargo-deny` → query "bans deny specific version syntax" → write the config against the current schema (`crate = "name@version"` vs the older `name`/`version` keys).
Not: write from memory, discover the schema changed when CI fails.

**Built-in check:**
Task: needs a bounded thread pool for a one-off parallel map.
Do: query the already-present dependency ("rayon scoped pool custom size") before writing anything.
Not: hand-roll a worker-channel loop next to an unused rayon feature.
