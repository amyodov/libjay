"""Dialect objects and per-dialect compiler construction.

Dialect settings (like APL's index origin) belong to a compiler instance,
never to global state.
"""

from __future__ import annotations

from dataclasses import dataclass

from . import _Lang


class J:
    """The J language. No dialect settings yet."""

    @dataclass(frozen=True)
    class Dialect:
        pass

    @staticmethod
    def create_compiler(dialect: "J.Dialect | None" = None) -> _Lang:
        return _Lang("j")


class APL:
    """The APL language."""

    @dataclass(frozen=True)
    class Dialect:
        index_base: int = 1

    @staticmethod
    def create_compiler(dialect: "APL.Dialect | None" = None) -> _Lang:
        base = dialect.index_base if dialect is not None else 1
        return _Lang("apl", index_origin=base)
