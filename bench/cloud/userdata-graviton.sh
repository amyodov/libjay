#!/bin/bash
# c7g.4xlarge — Graviton3, 16 vCPU, aarch64.
#
# Two firsts in one run.
#
#   The first ARM numbers. Every table in bench/README.md was taken on one
#   x86 laptop; the SIMD section says of aarch64 only that NEON is in the
#   baseline, so the two rungs there are expected to be the same code. This
#   measures whether they are, and what the whole of bench/ looks like on a
#   machine with real memory bandwidth — which is what most of those tables
#   turned out to be bounded by.
#
#   The first execution of the linux-aarch64 wheel. publish.yml
#   cross-compiles it and marks that leg `smoke: false`, because the x86
#   runner cannot run what it just built. This is the machine that can, so
#   the Python suite runs here too, against the wheel rather than a local
#   build: it is the only place that artifact is ever executed before a user
#   executes it.
#
# What is NOT here: SVE. The ladder has no rung above the aarch64 baseline,
# so a Graviton3's 256-bit SVE is invisible to libjay today; it is a
# multiversion rung of its own and belongs to a later phase. This run's
# baseline-vs-native table is the evidence for how much that rung is worth.

set -uo pipefail
#@@CONFIG@@
#@@COMMON@@

phase inputs fetch_inputs
phase python install_python
phase libjay install_libjay

arm_features() { grep -om1 'Features.*' /proc/cpuinfo || echo "NONE"; }
phase cpu-features arm_features

# The cross-compiled wheel, executed and then tested. pyarrow and pandas are
# what the Python suite wants beyond the bench rivals.
phase wheel-suite bash -c 'cd "$1" && VIRTUAL_ENV="$1/.venv" uv pip install pytest pyarrow pandas >/dev/null &&
	"$1/.venv/bin/python" -m pytest python/tests -q' _ "$WORK"

# Two rungs, not four: aarch64 has no v2 or v3, and a pinned one would clamp
# to the baseline and quote the same code twice under two names.
phase simd-levels py bench/simd.py --levels baseline,native --threads 1,16 "${BENCH_ARGS[@]}"

phase rust-toolchain install_rust
phase simd-equivalence crun test -p libjay --release --test simd -- --nocapture
phase rust-suite crun test -p libjay --release

phase bench py bench/bench.py --threads 16 "${BENCH_ARGS[@]}"
phase sweep py bench/sweep.py --threads 16
phase timeseries py bench/timeseries.py --threads 16 "${BENCH_ARGS[@]}"
phase windows py bench/windows.py
phase workloads py bench/workloads.py "${WORKLOAD_ARGS[@]}"
[ -f "$WORK/bench/workloads.md" ] && cp "$WORK/bench/workloads.md" "$RESULTS/workloads.md"

step "all phases attempted; finish() uploads and terminates"
exit 0
