#!/bin/bash
# c7i.4xlarge — Sapphire Rapids, 16 vCPU, AVX-512.
#
# The reason this profile exists: bench/README.md's "SIMD dispatch" section
# has carried "x86-64-v4 is built and symbol-checked but has never run" since
# the MSRV moved to 1.89. This is the machine that runs it. Two halves, and
# they want different artifacts:
#
#   the numbers      LIBJAY_CPU_LEVEL=v4 against v3, v2 and baseline, from
#                    the wheel publish.yml built — the artifact a user gets
#   the correctness  tests/simd.rs, which holds every level to identical
#                    elementwise results and 1e-12 on reductions, and which
#                    is a Rust test, so this profile carries a compiler
#
# Everything else in bench/ runs too, because a 16-vCPU x86 box is also the
# first honest multi-core number this project has: every table in
# bench/README.md was taken on a four-core 2017 laptop.

set -uo pipefail
#@@CONFIG@@
#@@COMMON@@

phase inputs fetch_inputs
phase python install_python
phase libjay install_libjay

# What the CPU actually has, before anything claims a level from it. If this
# line is empty the run is on the wrong instance type and every v4 figure
# below would silently be a clamped v3 one.
avx512_flags() { grep -om1 'avx512[a-z0-9_ ]*' /proc/cpuinfo || echo "NONE"; }
phase cpu-features avx512_flags

# The rung ladder, from the shipped wheel. v4 is why we are here; baseline,
# v2 and v3 are what it has to be read against, and the clamp means a level
# the CPU cannot run never appears as a number.
phase simd-levels py bench/simd.py --levels baseline,v2,v3,v4 --threads 1,16 "${BENCH_ARGS[@]}"

# The equivalence battery. `--nocapture` because the test prints which levels
# the machine let it compare, and on this machine that line is the record
# that the v4 clones were exercised at all.
phase rust-toolchain install_rust
phase simd-equivalence crun test -p libjay --release --test simd -- --nocapture
phase rust-suite crun test -p libjay --release

# The rest of bench/, on sixteen cores.
phase bench py bench/bench.py --threads 16 "${BENCH_ARGS[@]}"
phase sweep py bench/sweep.py --threads 16
phase timeseries py bench/timeseries.py --threads 16 "${BENCH_ARGS[@]}"
phase windows py bench/windows.py
phase workloads py bench/workloads.py "${WORKLOAD_ARGS[@]}"
[ -f "$WORK/bench/workloads.md" ] && cp "$WORK/bench/workloads.md" "$RESULTS/workloads.md"

step "all phases attempted; finish() uploads and terminates"
exit 0
