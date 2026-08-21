---
name: releasing
description: Cut a libjay release - preconditions, version bump, git tag, GitHub release, the wheel matrix, and PyPI. Invoked by the user only; a release never happens on the assistant's own initiative.
argument-hint: [version]
disable-model-invocation: true
---

# Release a version of libjay

One release reaches PyPI (a matrix of native wheels plus an sdist),
crates.io (the `libjay` core crate, via OIDC trusted publishing — see step
5), the git tag, and the GitHub release. Releases only happen when the user
asks for one — `disable-model-invocation` enforces that, and nothing in this
file overrides it. The version being released is `$1` (e.g. `0.2.0`): a bare
semver, no leading `v` — the tag adds it.

The publish pipeline is: GitHub release → `.github/workflows/publish.yml`
(maturin wheel matrix: linux x86_64/aarch64, macOS x86_64/aarch64, windows
x64, all abi3-py310, plus sdist) → PyPI trusted publisher (OIDC, no tokens).
This skill's job is everything around that trigger, in order, aborting
loudly at the first failure rather than improvising past it.

## 1. Preconditions

- Working tree clean, on `main`, in sync with `origin/main` (`git status -sb`).
- `$1` is not already released: absent from `git tag`, from
  `https://pypi.org/pypi/libjay/json`, and from
  `https://crates.io/api/v1/crates/libjay` (send a User-Agent or it 403s).
- `cargo publish -p libjay --dry-run` packages cleanly.
- The full local suite passes, including the recorded differential corpus
  (a replay: no interpreter is run):
  `cargo test -p libjay -p libjay-capi` and
  `cargo clippy --all-targets -- -D warnings`.
- The corpus still matches the live references — the oracles must be present
  (see CLAUDE.md): `cargo run -p libjay-devtools -- record j --check` and
  `... record apl --check`.
- The Python suite passes against a fresh build:
  `.venv/bin/maturin develop -q && .venv/bin/pytest python/tests -q`.
- CI is green on HEAD: `gh run list --branch main --limit 1`. Local tests
  cannot vouch for Windows or Linux; only CI can, and the wheel matrix will
  meet those platforms for real during publish.

## 2. Version bump

The single version source is `[workspace.package] version` in the root
`Cargo.toml` — the wheel (pyproject has `dynamic = ["version"]`), the crates,
`jay.__version__`, and `jay_version()` in the C ABI all read it from there.

A 0.1.x patch bump is the normal shape of a release right now: most releases
are a coverage wave (more valences, more corpus) rather than a breaking or
feature change, and semver's patch slot is where those land pre-1.0.

- Set `version = "$1"` in the root `Cargo.toml`.
- `cargo check -q` so `Cargo.lock` follows — **it must be committed with the
  bump**; a release built from a stale lockfile is not the tree that was
  tested.
- Add `$1`'s section to `CHANGELOG.md`, above `## Unreleased` (which stays,
  emptied back to its placeholder headings) and above the previous version.
- Write `docs/release-notes-$1.md`: what changed and what it displaces, same
  prose rules as everything else — this file is also the `gh release create`
  body (step 3).
- Commit `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md` and the release-notes
  file together. The version in the artifacts and the version being tagged
  must be the same string.

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

The tag may already exist at HEAD — this happened for 0.1.0, when the tag
was created before this step ran. Check first, and skip creation rather than
failing on "tag already exists":

```bash
if git rev-parse -q --verify "refs/tags/v$1" >/dev/null; then
  # It's only safe to reuse: the tag must point at HEAD, and HEAD's CI (just
  # checked above) must be green. If the tag points anywhere else, stop —
  # that is a real conflict, not this shortcut.
  [ "$(git rev-parse v$1)" = "$(git rev-parse HEAD)" ] || { echo "v$1 exists but not at HEAD" >&2; exit 1; }
  echo "v$1 already at HEAD with green CI — skipping tag creation"
else
  git tag v$1 && git push origin v$1
fi
gh release create v$1 --title "v$1 — <short summary>" --notes-file docs/release-notes-$1.md
```

Release-notes files use one line per paragraph — GitHub renders
hard-wrapped prose as ragged lines (the repo's 72-column habit does not
apply there). The title names WHAT THIS RELEASE CHANGES, not what libjay is: the
project description belongs on the repo, the title is the one-liner a
reader scans in the releases list (`v0.2.0 — AVX-512, column-major
DataFrames, APL trains`, not `v0.2.0 — independent J and APL…`). Lead with
the largest user-visible change; three items at most; no marketing.

Creating the release is the publish trigger — after this line, the release
is happening.

## 4. Watch and verify

The wheel matrix takes a while (five native builds plus an sdist):

```bash
gh run watch $(gh run list --workflow publish.yml --limit 1 --json databaseId -q '.[0].databaseId') --exit-status
```

Then confirm the release is real from a user's seat — every check below
proved useful catching a real gap after 0.1.0, so run all of them, not just
the first that passes:

- `gh release view v$1 --json assets -q '.assets | length'` → `10` (five
  wheels, one sdist, four C ABI bundles).
- `curl -s https://pypi.org/pypi/libjay/json` → `info.version == "$1"`, and
  the file list shows five wheels + one sdist.
- `curl -s https://crates.io/api/v1/crates/libjay | python3 -c "import json,sys; print(json.load(sys.stdin)['crate']['max_version'])"`
  → `$1`.
- `curl -s -o /dev/null -w '%{http_code}' https://docs.rs/libjay/$1/jay/` →
  `200`. docs.rs builds after crates.io accepts the upload, not
  instantly — allow up to ~30 minutes before treating a non-200 as a
  failure.
- Cold-run the real user path, from a clean cache so nothing is left over
  from testing an earlier version:
  `uvx --refresh libjay -e '(+/ % #) 1 2 3 4'` → `2.5`, and
  `uvx --refresh libjay -e "⎕←'hello'" --lang apl` → `hello`.

## 5. crates.io

Steady state (from 0.1.1 on): the `crates` job in publish.yml publishes the
core crate with the same version, via crates.io trusted publishing (OIDC —
no token stored anywhere), in the `crates` GitHub environment, after PyPI
succeeds. It runs by itself; nothing in this step is a manual action anymore
except the one-time item below. Gated on the repo variable
`CRATES_PUBLISH == "true"`, which is already set.

- [ ] **One-time owner action, not yet confirmed done**: register the
  trusted publisher on crates.io — Settings → Trusted Publishing → GitHub
  `amyodov/libjay`, workflow `publish.yml`, environment `crates`. Until this
  is confirmed, treat the `crates` job as unverified even though it will
  run: check its result after this release, and once a real OIDC publish has
  succeeded, delete this checklist line and the paragraph above it that
  hedges on it.

Bootstrap (already done, 0.1.0 only, kept here as history): crates.io cannot
attach a trusted publisher to a crate that does not exist, so the very first
publish was manual — `cargo publish -p libjay` with the owner's token, which
was then removed locally.

The binding crate (`libjay-python`) and the C ABI crate (`libjay-capi`) stay
`publish = false`: wheel users get the binding inside the wheel, C users
need artifacts and a header rather than a crate. Revisit capi publishing if
someone asks for `cargo add libjay-capi`.

## 6. Not part of this pipeline

Context7 indexing: revisit once docs/ is worth indexing, so AI assistants
answer from the current release rather than a stale crawl.
