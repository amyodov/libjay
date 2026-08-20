"""Device placement from Python.

Everything here needs a GPU adapter, so everything here skips without one:
a CI runner has none, and the CPU path these tests compare against is
covered by the rest of the suite anyway.
"""

import numpy as np
import pytest

import jay
from jay import JayError

ADAPTERS = jay.devices()
needs_gpu = pytest.mark.skipif(not ADAPTERS, reason="no GPU adapter on this machine")

# libjay computes in f64 and will not lose that quietly, so on an adapter
# without f64 in shaders the only thing that reaches the device is the
# opted-in f32 path. The tolerance follows the precision.
PRECISION = "f64" if ADAPTERS and ADAPTERS[0].f64 else "f32"
REL = 1e-14 if PRECISION == "f64" else 1e-4

# Above jay.device.MIN_ELEMS, so the kernel is worth a dispatch.
N = 1_000_000


@pytest.fixture(scope="module")
def data():
    rng = np.random.default_rng(20260820)
    return rng.random(N) + 0.5, rng.random(N) + 0.5


def close(a, b):
    scale = max(abs(a), abs(b), 1.0)
    return abs(a - b) <= REL * scale


class TestDevices:
    def test_listing_never_fails(self):
        for d in jay.devices():
            assert d.name
            assert isinstance(d.f64, bool)

    def test_an_undeployed_kernel_has_no_device(self):
        k = jay.j.compile("+/ {w} * {x}")
        assert k.device is None
        assert k.precision is None

    @needs_gpu
    def test_deploy_names_the_adapter(self):
        k = jay.j.compile("+/ {w} * {x}").deploy("gpu", precision=PRECISION)
        assert k.device is not None
        assert k.device.name == jay.devices()[0].name
        assert k.precision == PRECISION

    def test_deploy_to_the_cpu_always_works(self, data):
        w, x = data
        src = "+/ {w} * {x}"
        plain = jay.j.compile(src, {"w": w, "x": x})
        cpu = plain.deploy("cpu")
        assert cpu.device is not None
        assert cpu() == plain()

    def test_an_unknown_device_is_refused(self):
        with pytest.raises(JayError, match="unknown device"):
            jay.j.compile("2 + 2").deploy("tpu")

    @needs_gpu
    def test_an_unknown_precision_is_refused(self):
        with pytest.raises(JayError, match="unknown precision"):
            jay.j.compile("2 + 2").deploy("gpu", precision="f16")


@needs_gpu
class TestPlacement:
    def test_the_device_computes_what_the_cpu_computes(self, data):
        w, x = data
        for src in ("+/ {w} * {x}", ">./ {w} * {x}", "+/ ({w} - {x}) * {w} - {x}"):
            plain = jay.j.compile(src, {"w": w, "x": x})
            gpu = plain.deploy("gpu", precision=PRECISION)
            assert close(plain(), gpu()), src

    def test_a_map_comes_back_as_an_ordinary_value(self, data):
        w, x = data
        src = "1 + {w} * {x}"
        plain = jay.j.compile(src, {"w": w, "x": x})
        gpu = plain.deploy("gpu", precision=PRECISION)
        a, b = plain(), gpu()
        assert a.shape == b.shape == (N,)
        assert close(a.tolist()[0], b.tolist()[0])

    def test_explain_says_where_it_ran(self, data):
        w, x = data
        k = jay.j.compile("+/ {w} * {x}", {"w": w, "x": x}).deploy(
            "gpu", precision=PRECISION
        )
        text = k.explain()
        assert jay.devices()[0].name in text
        assert "device: gpu" in text or "device: cpu" in text

    def test_an_integer_chain_says_why_it_stayed(self):
        v = np.arange(N, dtype=np.int64)
        k = jay.j.compile("+/ {x} * {x}", {"x": v}).deploy("gpu", precision=PRECISION)
        assert "64-bit integers" in k.explain()


@needs_gpu
class TestResidency:
    def test_an_uploaded_array_reads_as_itself(self, data):
        _, x = data
        k = jay.j.compile("+/ {x}").deploy("gpu", precision=PRECISION)
        up = k.upload(x)
        assert isinstance(up, jay.DeviceArray)
        assert up.shape == (N,)
        assert up.dtype == "float"
        assert up.resident
        assert "DeviceArray" in repr(up)
        assert np.allclose(np.asarray(up.download().tolist()), x)

    def test_a_resident_argument_gives_the_same_answer(self, data):
        w, x = data
        src = "+/ {w} * {x}"
        plain = jay.j.compile(src, {"w": w, "x": x})
        gpu = jay.j.compile(src).deploy("gpu", precision=PRECISION)
        pinned = gpu.bind({"w": gpu.upload(w), "x": gpu.upload(x)})
        assert close(plain(), pinned())
        # Twice: the second call has nothing left to upload.
        assert pinned() == pinned()

    def test_keep_on_device_returns_a_device_array(self, data):
        w, x = data
        gpu = jay.j.compile("1 + {w} * {x}").deploy("gpu", precision=PRECISION)
        out = gpu({"w": w, "x": x}, keep_on_device=True)
        assert isinstance(out, jay.DeviceArray)
        assert out.shape == (N,)
        assert out.resident
        plain = jay.j.compile("1 + {w} * {x}")({"w": w, "x": x})
        assert close(plain.tolist()[0], out.download().tolist()[0])

    def test_keep_on_device_needs_a_device(self, data):
        _, x = data
        k = jay.j.compile("+/ {x}", {"x": x})
        with pytest.raises(JayError, match="not deployed"):
            k(keep_on_device=True)

    def test_uploading_needs_a_device(self, data):
        _, x = data
        with pytest.raises(JayError, match="not deployed"):
            jay.j.compile("+/ {x}").upload(x)
