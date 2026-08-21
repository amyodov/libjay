#!/bin/bash
# g5.xlarge — NVIDIA A10G, 4 vCPU, on the Deep Learning Base AMI (Ubuntu
# 22.04) so the driver is already installed and nothing builds a kernel
# module here.
#
# This is the run docs/decisions.md's phase-7 entry asks for by name: "no f64
# shader has run anywhere … it needs a Linux/Vulkan or Windows/DX12 box with
# an adapter that reports SHADER_F64". NVIDIA's Vulkan driver reports it, so
# this profile is both the first GPU f64 measurement and the validation that
# the generated WGSL — which naga has only ever type-checked — computes what
# the CPU computes.
#
# The order below is deliberate: the cheapest, most informative phase runs
# first. If the adapter turns out not to report f64 after all, `adapters`
# says so in the first two minutes and every later phase's failure is already
# explained.
#
# Four vCPU is the constraint that shapes the rest: the Rust battery's build
# is the long pole here, not the measurement, which is why the Python
# equivalence check runs before it rather than after.

set -uo pipefail
#@@CONFIG@@
#@@COMMON@@

# wgpu needs the Vulkan loader; the driver on this AMI supplies the ICD. No
# display is involved — a headless Vulkan device is what libjay asks for.
install_vulkan() {
	pkg libvulkan1 vulkan-tools libnvidia-gl-535 || pkg libvulkan1 vulkan-tools || return 1
	nvidia-smi || true
	ls -l /usr/share/vulkan/icd.d/ || true
	vulkaninfo --summary 2>&1 | head -60
	vulkaninfo 2>/dev/null | grep -i -m2 'shaderFloat64' || {
		printf 'no shaderFloat64 in vulkaninfo: the f64 path will decline to the CPU\n'
		return 1
	}
}

phase vulkan install_vulkan
phase inputs fetch_inputs
phase python install_python
phase libjay install_libjay

# The headline, in one line: what libjay sees, and whether it sees f64.
phase adapters py -c 'import jay
ds = jay.devices()
if not ds:
    raise SystemExit("no adapter: wgpu found nothing to run on")
for d in ds:
    print(f"{d.name} ({d.backend}, {d.kind}) f64 in shaders: {d.f64}")
if not ds[0].f64:
    raise SystemExit("adapter has no f64 in shaders; this profile has nothing new to measure")
print("SHADER_F64 present: the f64 device path is about to execute for the first time")'

# Equivalence before speed. libjay computes in f64 on both processors, so a
# map is contracted to be bit-identical and a reduction to 1e-14; this is the
# first time that contract has been tested against a real f64 shader.
phase device-equivalence-python py -c 'import numpy as np, jay
rng = np.random.default_rng(20260822)
x = rng.standard_normal(4_000_000); w = rng.standard_normal(4_000_000)
cases = {
    "map":       "{w} * {x} + 0.5",
    "reduce":    "+/ {w} * {x}",
    "poly":      "+/ 2.5 + {x} * 1.5 + {x} * 0.5 * {x}",
    "chain":     "({x} * {w}) + {x} - 0.5",
}
for name, src in cases.items():
    k = jay.j.compile(src).bind({"w": w, "x": x})
    cpu = k()
    gpu = k.deploy("gpu")()
    a, b = np.asarray(cpu, dtype=float), np.asarray(gpu, dtype=float)
    scale = max(1.0, float(np.max(np.abs(a))))
    err = float(np.max(np.abs(a - b))) / scale
    exact = bool(np.array_equal(a, b))
    print(f"{name:10s} max rel {err:.3e}  bit-identical: {exact}")
    assert err < 1e-13, f"{name} disagrees by {err}"
print("f64 CPU and f64 GPU agree")'

# The measurement, at the size bench/README.md quotes, so the row is directly
# comparable with the Metal f32 table already there.
phase device-bench py bench/device.py "${BENCH_ARGS[@]}"

# The real battery. It skips cleanly with no adapter, which is how CI stays
# green; here it should skip nothing.
phase rust-toolchain install_rust
phase device-equivalence-rust crun test -p libjay --release --test device -- --nocapture

# Four cores of context for the device columns.
phase bench py bench/bench.py --threads 4 "${BENCH_ARGS[@]}"

step "all phases attempted; finish() uploads and terminates"
exit 0
