"""Template (PEP 750 t-string) support. Python 3.14+ only; imported lazily
so the rest of the package works on 3.10."""

from __future__ import annotations

from string.templatelib import Template


def split_template(t: Template) -> tuple[list[str], list[str], dict]:
    """Split a template into literal parts, hole names, and default values.

    Interpolations must be plain identifiers: rebinding is keyed on names,
    and an expression like ``df['close']`` has none. Such values should be
    assigned to a variable first, or passed via the string form with an
    explicit data dict.
    """
    parts = list(t.strings)
    names: list[str] = []
    values: dict = {}
    for interp in t.interpolations:
        expr = interp.expression
        if not expr.isidentifier():
            raise TypeError(
                f"interpolation {{{expr}}} is not a plain identifier; "
                "assign it to a variable first, or use the string form "
                "with an explicit data dict"
            )
        names.append(expr)
        values[expr] = interp.value
    return parts, names, values
