"""Embedded J and APL.

The surface follows the `re` module's shape: ``libjay.j(...)`` compiles,
binds and executes in one call; ``libjay.j.compile(...)`` returns a reusable
kernel. ``libjay.apl`` is the same thing for APL.
"""

from __future__ import annotations

import sys

from . import _jay

__all__ = ["j", "apl", "compile", "Kernel", "JayError"]
__version__ = _jay.__version__

JayError = _jay.JayError

_HAVE_TSTRINGS = sys.version_info >= (3, 14)


def _write_stdout(text: str) -> None:
    sys.stdout.write(text)


class Kernel:
    """A compiled program together with default values for its parameters.

    Immutable: :meth:`bind` returns a new kernel; the compiled program is
    shared and reusable across threads.
    """

    __slots__ = ("_inner", "_defaults")

    def __init__(self, inner, defaults: dict):
        self._inner = inner
        self._defaults = defaults

    @property
    def params(self) -> list[str]:
        """Parameter names, in positional order."""
        return list(self._inner.params)

    def bind(self, data: dict) -> "Kernel":
        """Return a new kernel with these values overriding current defaults."""
        self._check_names(data)
        return Kernel(self._inner, {**self._defaults, **data})

    def __call__(self, data: dict | None = None):
        """Execute; call-time values override bound ones. Returns the value
        of the last sentence, or None when it yields no value."""
        values = self._defaults if not data else {**self._defaults, **self._check_names(data)}
        missing = [p for p in self._inner.params if p not in values]
        if missing:
            raise JayError(
                "missing value(s) for parameter(s): " + ", ".join(missing)
            )
        ordered = [values[p] for p in self._inner.params]
        result, _ = self._inner.run(ordered, _write_stdout, False)
        return result

    def run_display(self, data: dict | None = None) -> str | None:
        """Execute like __call__, but return the last value formatted for
        display (None when there is no value). Used by the CLI."""
        values = self._defaults if not data else {**self._defaults, **self._check_names(data)}
        ordered = [values[p] for p in self._inner.params]
        _, display = self._inner.run(ordered, _write_stdout, True)
        return display

    def _check_names(self, data: dict) -> dict:
        unknown = [k for k in data if k not in self._inner.params]
        if unknown:
            raise JayError(
                "unknown parameter(s): "
                + ", ".join(unknown)
                + "; this expression has: "
                + (", ".join(self._inner.params) or "none")
            )
        return data


class _Lang:
    """One language's entry point: callable for the one-shot form, with
    ``compile`` for the kernel form."""

    __slots__ = ("_name", "_index_origin")

    def __init__(self, name: str, index_origin: int | None = None):
        self._name = name
        self._index_origin = index_origin

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
            inner = _jay.compile_parts(self._name, parts, names, origin)
        elif isinstance(source, str):
            inner = _jay.compile(self._name, source, origin)
        else:
            raise TypeError(f"expected str or Template, got {type(source).__name__}")
        kernel = Kernel(inner, defaults)
        return kernel.bind(data) if data else kernel

    def __call__(self, source, data: dict | None = None, **opts):
        """Compile, bind and execute in one call; returns the value."""
        return self.compile(source, data, **opts)()


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
