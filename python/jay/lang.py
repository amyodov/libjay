"""Dialect objects and per-dialect compiler construction.

Dialect settings (like APL's index origin) belong to a compiler instance,
never to global state.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

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
    """The APL language.

    libjay ships the APL2/ISO line that GNU APL embodies and that the
    differential suite verifies. The settings below are the points where
    the APL lineages differ; each defaults to that line's reading, and
    ``Dialect.dyalog`` is the preset that names the other one wherever
    libjay implements it. A setting libjay does not implement both
    readings of is a "not implemented yet" error from the compiler rather
    than a silently different answer. ``trains`` ships on in both presets,
    as an extension: refusing a feature the oracle merely lacks serves
    nobody.
    """

    @dataclass(frozen=True)
    class Dialect:
        index_base: int = 1
        """APL's ``⎕IO``."""

        comparison_tolerance: float | None = None
        """``⎕CT``; None is the language's own default."""

        nested_model: str = "floating"
        """"floating" (a simple scalar cannot be nested) or "grounded"."""

        first_disclose: str = "up-is-first"
        """"up-is-first" (``↑`` first, ``⊃`` disclose) or "up-is-mix"."""

        index_form: str = "scalar-per-axis"
        """What ``⌷`` indexes with: "scalar-per-axis" (one index per axis,
        all of them named) or "axis-vectors" (the leading axes, so a
        shorter index takes the trailing ones whole)."""

        partition: str = "flags"
        """What a dyadic ``⊂`` does: "flags" (a partition begins where the
        left argument rises, and a zero drops its item) or "counts"
        (Dyalog's partitioned enclose). ``⊆`` is the flag reading in
        both."""

        depth_sign: str = "unsigned"
        """What ``≡`` answers for an array whose items differ in depth:
        "unsigned" or "signed" (the depth negated)."""

        dfn_result: str = "last-sentence"
        """Which sentence of a dfn answers: "last-sentence" or
        "first-non-assignment"."""

        default_arg: str = "eager"
        """When ``⍺←v`` evaluates ``v``: "eager" or "lazy"."""

        complex_order: str = "real-then-imaginary"
        """How a grade orders complex values: "real-then-imaginary" or
        "magnitude-then-angle"."""

        nested_grade: str = "apl2"
        """How a grade orders nested items: "apl2" (rank, then shape, then
        the atoms, characters before numbers before nested values) or
        "total-order" (Dyalog's total array ordering)."""

        lookup_left: str = "any-rank"
        """What dyadic ``⍳`` takes on its left: "any-rank" (the items of a
        left argument of any rank are searched) or "vector-only" (Dyalog's
        rule, where anything but a vector is a rank error)."""

        gcd_rule: str = "tolerant"
        """Which line's ``∨`` and ``∧``: "tolerant" (GNU APL — a zero
        argument hands its partner back with the sign, so ``¯3∨0`` is
        ``¯3``, and a near-whole or vanishing argument is rounded first) or
        "exact" (Dyalog and J — the magnitude, and the values as given)."""

        near_count: str = "absolute"
        """How a float merely NEAR a whole number is admitted where a
        count, a length or an index belongs: "absolute" (GNU APL — a flat
        ``1E¯10`` at every magnitude) or "tolerant" (Dyalog — relative, and
        scaled by ``⎕CT``). It is not the comparison tolerance: ``⎕CT←0``
        leaves either window where it is."""

        floor_rule: str = "shift"
        """How ``⌊`` and ``⌈`` read a value just short of an integer:
        "shift" (GNU APL — ``⌊y+⎕CT``, an absolute step) or "scaled"
        (Dyalog — ``⌊y+⎕CT×1⌈|y``, a step that grows with the
        magnitude)."""

        encode_digits: str = "tolerant"
        """Whether ``⊤`` takes its digits with the tolerant residue
        ``|`` uses: "tolerant" (GNU APL) or "exact" (Dyalog, which leaves
        ``2 2⊤4-1E¯14`` as ``1 2`` rather than ``0 0``)."""

        trains: bool = True
        """Whether a function may stand where a value belongs: a run of
        functions is then a train, and ``F←+/`` names one. Ships on, as an
        extension GNU APL has neither spelling of; False is the strict
        reading, where both are a syntax error."""

    # The presets. `gnu` is the default: the APL2/ISO line the oracle
    # verifies. `dyalog` is as much of the Dyalog line as libjay answers
    # today — docs/coverage.md says what it still leaves to the other
    # reading, and docs/status.md counts what the recording still holds
    # against it.
    Dialect.gnu = Dialect()
    Dialect.dyalog = Dialect(
        comparison_tolerance=1e-14,
        first_disclose="up-is-mix",
        index_form="axis-vectors",
        partition="counts",
        depth_sign="signed",
        dfn_result="first-non-assignment",
        nested_grade="total-order",
        lookup_left="vector-only",
        gcd_rule="exact",
        near_count="tolerant",
        floor_rule="scaled",
        encode_digits="exact",
    )

    @staticmethod
    def create_compiler(dialect: "APL.Dialect | None" = None) -> _Lang:
        d = dialect if dialect is not None else APL.Dialect.gnu
        settings = asdict(d)
        return _Lang("apl", index_origin=settings.pop("index_base"), **settings)
