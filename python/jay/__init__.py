"""Embedded J and APL.

The surface follows the `re` module's shape: ``jay.j(...)`` compiles,
binds and executes in one call; ``jay.j.compile(...)`` returns a reusable
kernel. ``jay.apl`` is the same thing for APL.
"""

from __future__ import annotations

import sys
from functools import lru_cache
from typing import NamedTuple

from . import _jay

__all__ = [
    "j",
    "apl",
    "compile",
    "devices",
    "Kernel",
    "Device",
    "DeviceArray",
    "JayError",
    "clear_cache",
]
__version__ = _jay.__version__

JayError = _jay.JayError
DeviceArray = _jay.DeviceArray


class Device(NamedTuple):
    """One adapter, as the machine reports it."""

    name: str
    backend: str
    kind: str
    f64: bool
    """Whether shaders on this adapter can compute in double precision."""


def devices() -> list[Device]:
    """Every GPU adapter this machine offers, best first.

    An empty list is the ordinary answer on a machine without one; nothing
    else changes, since every expression already runs on the CPU.
    """
    return [Device(*d) for d in _jay.devices()]

_HAVE_TSTRINGS = sys.version_info >= (3, 14)

# Compiling the same string twice gives the same program, and a compiled
# program is immutable, holds no data and is safe to share between threads —
# so the second compilation of a source that has already been seen is
# answered from here. The cache is this process's only; nothing is written
# to disk. It is keyed by everything the compiler reads, and bounded, so a
# caller that generates source text cannot grow it without limit.
_CACHE_SIZE = 512


# The dialect settings a compiler carries besides the index origin, in the
# order the extension takes them. Each is None for "the shipped default";
# `jay.lang.APL.Dialect` is the named form of the same thing.
_DIALECT_KEYS = (
    "comparison_tolerance",
    "nested_model",
    "first_disclose",
    "index_form",
    "partition",
    "depth_sign",
    "dfn_result",
    "default_arg",
    "complex_order",
    "nested_grade",
    "lookup_left",
    "gcd_rule",
    "near_count",
    "floor_rule",
    "encode_digits",
    "inner_each",
    "control_strictness",
    "trains",
)


@lru_cache(maxsize=_CACHE_SIZE)
def _compiled(lang: str, source: str, origin, dialect: tuple = ()):
    return _jay.compile(lang, source, origin, *dialect)


@lru_cache(maxsize=_CACHE_SIZE)
def _compiled_parts(lang: str, parts: tuple, names: tuple, origin, dialect: tuple = ()):
    return _jay.compile_parts(lang, list(parts), list(names), origin, *dialect)


def clear_cache() -> None:
    """Forget every compiled program this process has cached."""
    _compiled.cache_clear()
    _compiled_parts.cache_clear()


def _write_stdout(text: str) -> None:
    sys.stdout.write(text)


def _read_stdin() -> str | None:
    """One line of this process's standard input, or None at its end.

    The sandbox opens stdin as it opens stdout, so this is the default for
    every call; whether stdin is a terminal or a pipe makes no difference.
    """
    try:
        return input()
    except EOFError:
        return None


class Kernel:
    """A compiled program together with default values for its parameters.

    Immutable: :meth:`bind` returns a new kernel; the compiled program is
    shared and reusable across threads.
    """

    __slots__ = ("_inner", "_defaults", "_params")

    def __init__(self, inner, defaults: dict):
        self._inner = inner
        self._defaults = defaults
        # The compiled program's parameter list does not change, and every
        # call reads it two or three times; read it across once instead.
        self._params = list(inner.params)

    @property
    def params(self) -> list[str]:
        """Parameter names, in positional order."""
        return list(self._params)

    def bind(self, data: dict) -> "Kernel":
        """Return a new kernel with these values overriding current defaults."""
        self._check_names(data)
        return Kernel(self._inner, {**self._defaults, **data})

    def deploy(self, device: str = "gpu", *, precision: str | None = None) -> "Kernel":
        """Return a new kernel whose fused chains run on `device`.

        Placement is not binding: the bound values, the result and every
        diagnostic are what they were, and whatever the device will not take
        runs on the CPU — :meth:`explain` names each and says why. `device`
        is "gpu" or "cpu".

        `precision` is "f64" (the default) or "f32". libjay computes in f64
        and most adapters have no f64 in shaders at all; on those, an f64
        kernel stays on the CPU rather than quietly losing precision, and
        "f32" is how you say you want single precision anyway.
        """
        return Kernel(self._inner.deploy(device, precision), dict(self._defaults))

    @property
    def device(self) -> Device | None:
        """The adapter this kernel is deployed on, or None for the CPU."""
        d = self._inner.device
        return None if d is None else Device(*d[:4])

    @property
    def precision(self) -> str | None:
        """The type this kernel's device computes in, or None for the CPU."""
        d = self._inner.device
        return None if d is None else d[4]

    def upload(self, value) -> DeviceArray:
        """`value` with its elements resident on this kernel's device.

        The result reads as an ordinary value and also carries the device
        allocation, so passing it back uploads nothing.
        """
        return self._inner.upload(value)

    def __call__(
        self,
        data: dict | None = None,
        *,
        keep_on_device: bool = False,
        input=_read_stdin,
    ):
        """Execute; call-time values override bound ones. Returns the value
        of the last sentence, or None when it yields no value.

        `input` is what an expression that reads (APL `⍞` and `⎕`, J
        `1!:1 ]1`) is answered with: a callable returning one line per call
        and None at the end of the input. The default reads this process's
        standard input; None attaches no source at all, and an expression
        that reads one then says so.

        `keep_on_device` returns the value as a :class:`DeviceArray` left
        resident on this kernel's device, so the next call that reads it
        uploads nothing. (The host copy is materialised at the same time:
        handing a result straight to the next kernel without touching the
        host is not implemented yet.)
        """
        values = self._defaults if not data else {**self._defaults, **self._check_names(data)}
        try:
            ordered = [values[p] for p in self._params]
        except KeyError:
            missing = [p for p in self._params if p not in values]
            raise JayError(
                "missing value(s) for parameter(s): " + ", ".join(missing)
            ) from None
        result, _ = self._inner.run(ordered, _write_stdout, False, keep_on_device, input)
        return result

    def explain(self, data: dict | None = None) -> str:
        """Describe what the expression became: one section per sentence,
        with the verb structure the frontend produced and the fused kernels
        the optimiser made of it.

        Values follow the same cascade as :meth:`__call__` — interpolated,
        bound, then call-time. When every parameter has one, the program is
        run and each node is annotated with the shape and dtype it
        produced; when one is missing, the structure is described alone.
        """
        values = self._defaults if not data else {**self._defaults, **self._check_names(data)}
        if any(p not in values for p in self._params):
            return self._inner.explain(None)
        return self._inner.explain([values[p] for p in self._params])

    def run_display(self, data: dict | None = None, *, input=_read_stdin) -> str | None:
        """Execute like __call__, but return the last value formatted for
        display (None when there is no value). Used by the CLI."""
        values = self._defaults if not data else {**self._defaults, **self._check_names(data)}
        ordered = [values[p] for p in self._params]
        _, display = self._inner.run(ordered, _write_stdout, True, False, input)
        return display

    def _check_names(self, data: dict) -> dict:
        unknown = [k for k in data if k not in self._params]
        if unknown:
            raise JayError(
                "unknown parameter(s): "
                + ", ".join(unknown)
                + "; this expression has: "
                + (", ".join(self._params) or "none")
            )
        return data


class _Lang:
    """One language's entry point: callable for the one-shot form, with
    ``compile`` for the kernel form."""

    __slots__ = ("_name", "_index_origin", "_dialect")

    def __init__(self, name: str, index_origin: int | None = None, **dialect):
        unknown = [k for k in dialect if k not in _DIALECT_KEYS]
        if unknown:
            raise TypeError(
                "unknown dialect setting(s): "
                + ", ".join(unknown)
                + "; this language has: "
                + ", ".join(_DIALECT_KEYS)
            )
        self._name = name
        self._index_origin = index_origin
        # A tuple, because it is part of the compilation cache's key.
        self._dialect = tuple(dialect.get(k) for k in _DIALECT_KEYS)

    @property
    def name(self) -> str:
        return self._name

    def compile(self, source, data: dict | None = None, *, index_origin: int | None = None) -> Kernel:
        """Compile a string (with ``{name}`` holes) or a t-string template.

        Interpolated/`data` values become both the type contract and the
        default values; the kernel keeps them alive.
        """
        origin = index_origin if index_origin is not None else self._index_origin
        defaults: dict = {}
        if _HAVE_TSTRINGS and _is_template(source):
            from ._tstring import split_template

            parts, names, defaults = split_template(source)
            inner = _compiled_parts(
                self._name, tuple(parts), tuple(names), origin, self._dialect
            )
        elif isinstance(source, str):
            inner = _compiled(self._name, source, origin, self._dialect)
        else:
            raise TypeError(f"expected str or Template, got {type(source).__name__}")
        kernel = Kernel(inner, defaults)
        return kernel.bind(data) if data else kernel

    def __call__(self, source, data: dict | None = None, *, input=_read_stdin, **opts):
        """Compile, bind and execute in one call; returns the value.

        `input` is the run's input source, as on :meth:`Kernel.__call__`.

        The shortcut runs on the CPU and has no device placement: there is
        nowhere in one call to say where, and uploading data for a single
        run would rarely pay for itself. Compile a kernel and
        :meth:`Kernel.deploy` it to use a device.
        """
        return self.compile(source, data, **opts)(input=input)


def _is_template(obj) -> bool:
    from string.templatelib import Template

    return isinstance(obj, Template)


j = _Lang("j")
apl = _Lang("apl")


def compile(source, data: dict | None = None, *, lang: str = "j", **opts) -> Kernel:
    """Module-level compile; ``lang`` selects the language by name."""
    handle = {"j": j, "apl": apl}.get(lang.lower())
    if handle is None:
        raise ValueError(f"unknown language: {lang!r} (expected 'j' or 'apl')")
    return handle.compile(source, data, **opts)
