---
name: releasing
description: Cut a libjay release - preconditions, version bump, git tag, GitHub release, the wheel matrix, and PyPI. Invoked by the user only; a release never happens on the assistant's own initiative.
argument-hint: [version]
disable-model-invocation: true
---

# Release a version of libjay

One release reaches PyPI (a matrix of native wheels plus an sdist), the git
tag, and the GitHub release. Releases only happen when the user asks for one
— `disable-model-invocation` enforces that, and nothing in this file
overrides it. The version being released is `$1` (e.g. `0.2.0`): a bare
semver, no leading `v` — the tag adds it.

The publish pipeline is: GitHub release → `.github/workflows/publish.yml`
(maturin wheel matrix: linux x86_64/aarch64, macOS x86_64/aarch64, windows
x64, all abi3-py310, plus sdist) → PyPI trusted publisher (OIDC, no tokens).
This skill's job is everything around that trigger, in order, aborting
loudly at the first failure rather than improvising past it.

## 1. Preconditions

- Working tree clean, on `main`, in sync with `origin/main` (`git status -sb`).
- `$1` is not already released: absent from `git tag` and from
  `https://pypi.org/pypi/libjay/json`.
- The full local suite passes, including the differential corpus against the
  reference J (the oracle must be present — see CLAUDE.md):
  `cargo test -p libjay -p libjay-capi` and
  `cargo clippy --all-targets -- -D warnings`.
- The Python suite passes against a fresh build:
  `.venv/bin/maturin develop -q && .venv/bin/pytest python/tests -q`.
- CI is green on HEAD: `gh run list --branch main --limit 1`. Local tests
  cannot vouch for Windows or Linux; only CI can, and the wheel matrix will
  meet those platforms for real during publish.

## 2. Version bump

The single version source is `[workspace.package] version` in the root
`Cargo.toml` — the wheel (pyproject has `dynamic = ["version"]`), the crates,
`jay.__version__`, and `jay_version()` in the C ABI all read it from there.

- Set `version = "$1"` in the root `Cargo.toml`.
- `cargo check -q` so `Cargo.lock` follows.
- Commit both together. The version in the artifacts and the version being
  tagged must be the same string.

## 3. Tag and release

Push the bump first, then **wait for its own CI run to go green before
tagging**. The green checked in step 1 was for the commit before the bump.

```bash
git push origin main
until [ "$(gh run list --branch main --limit 1 --json status --jq '.[0].status')" = completed ]; do sleep 20; done
gh run list --branch main --limit 1 --json headSha,conclusion --jq '.[0] | "\(.headSha[0:7]) \(.conclusion)"'
```

That has to print the bump commit and `success`. If it does not, stop: the
tag is the point of no return.

```bash
git tag v$1 && git push origin v$1
gh release create v$1 --title "v$1 — <short summary>" --notes "<what changed and why it matters>"
```

Release notes follow the prose rules in CLAUDE.md: what changed and what it
displaces, not a file list. Creating the release is the publish trigger —
after this line, the release is happening.

## 4. Watch and verify

The wheel matrix takes a while (five native builds plus an sdist):

```bash
gh run watch $(gh run list --workflow publish.yml --limit 1 --json databaseId -q '.[0].databaseId') --exit-status
```

Then confirm the release is real from a user's seat:

- `curl -s https://pypi.org/pypi/libjay/json` → `info.version == "$1"`, and
  the file list shows five wheels + one sdist.
- Cold-run the real user path:
  `uvx --refresh libjay -e '(+/ % #) 1 2 3 4'` → `2.5`, and
  `uvx --refresh libjay -e "⎕←'hello'" --lang apl` → `hello`.

## 5. Not part of this pipeline (yet)

- **crates.io** (`libjay` core crate) is a separate, manual decision — the
  name is reserved-by-availability only; publishing the Rust crate needs a
  `cargo publish` review pass of its own.
- No MCP registry, no Context7 — libjay is a library, not an MCP server;
  revisit Context7 once docs/ is worth indexing.
