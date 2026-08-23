"""The `libjay` command: run a J or APL expression or file."""

from __future__ import annotations

import argparse
import sys

from . import JayError, __version__, apl, j

_EXTENSIONS = {
    ".ijs": j,
    ".j": j,
    ".apl": apl,
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="libjay",
        description="Run a J or APL expression or source file.",
    )
    parser.add_argument("file", nargs="?", help="source file (.ijs/.j for J, .apl for APL)")
    parser.add_argument("-e", "--expr", metavar="EXPR", help="run this expression")
    parser.add_argument(
        "--lang",
        choices=["j", "apl"],
        help="language; default: by file extension, or J for -e",
    )
    parser.add_argument(
        "--dialect",
        choices=["gnu", "dyalog"],
        help="APL dialect: the APL2/GNU line (default) or Dyalog's; APL only",
    )
    parser.add_argument(
        "--explain",
        action="store_true",
        help="print what the expression became instead of running it",
    )
    parser.add_argument("--version", action="version", version=f"libjay {__version__}")
    args = parser.parse_args(argv)

    if (args.file is None) == (args.expr is None):
        parser.error("give either a source file or -e EXPR")

    if args.expr is not None:
        source = args.expr
        lang = {"j": j, "apl": apl}[args.lang or "j"]
    else:
        try:
            with open(args.file, encoding="utf-8") as f:
                source = f.read()
        except OSError as e:
            print(f"libjay: {e}", file=sys.stderr)
            return 1
        if args.lang:
            lang = {"j": j, "apl": apl}[args.lang]
        else:
            ext = "." + args.file.rsplit(".", 1)[-1].lower() if "." in args.file else ""
            lang = _EXTENSIONS.get(ext)
            if lang is None:
                print(
                    f"libjay: cannot tell the language of {args.file!r}; use --lang",
                    file=sys.stderr,
                )
                return 1

    if args.dialect and lang is not apl:
        parser.error("--dialect applies to APL only")
    try:
        if args.dialect == "dyalog":
            from .lang import APL

            compiler = APL.create_compiler(APL.Dialect.dyalog)
            kernel = compiler.compile(source)
        else:
            kernel = lang.compile(source)
        if args.explain:
            print(kernel.explain().rstrip("\n"))
            return 0
        display = kernel.run_display()
    except JayError as e:
        print(str(e), file=sys.stderr)
        return 1
    if display is not None:
        print(display)
    return 0


if __name__ == "__main__":
    sys.exit(main())
