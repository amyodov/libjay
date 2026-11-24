"""Check that a libjay wheel has LLVM compiled in, and that it runs.

    check_wheel.py --contents WHEEL...   inspect built wheels, before install
    check_wheel.py                       check the installed package

The first form prints each wheel's size and fails unless the wheel carries
LLVM's licence and no LLVM shared library (LLVM must be linked statically).
The second fails unless the installed extension reports both code generators
and a fold, a rank-0 body and a fused map forced through LLVM
(LIBJAY_JIT=llvm, read once per process, hence the child interpreters)
answer to the bit what the interpreter answers, with nothing on standard
error.
"""

import os
import subprocess
import sys
import zipfile

SENTENCES = [
    "0 (] F.. (4 : 'y + 0.5 * x - y')) 0.001 * i. 5000",
    "(3 : 'if. y > 2.5 do. y * y else. y - 1 end.')\"0 ] 0.001 * i. 5000",
    "+/ 1 + 2 * 0.5 * i. 1000000",
]


def contents(wheels: list[str]) -> int:
    if not wheels:
        print("FAIL: no wheel given", file=sys.stderr)
        return 1
    for wheel in wheels:
        names = zipfile.ZipFile(wheel).namelist()
        size = os.path.getsize(wheel) / 2**20
        print(f"{os.path.basename(wheel)}: {size:.1f} MiB")
        if not any(n.endswith("licenses/LICENSE-LLVM") for n in names):
            print(f"FAIL: no LICENSE-LLVM in {wheel}", file=sys.stderr)
            return 1
        shared = [n for n in names if "libLLVM" in n or "LLVM.dll" in n]
        if shared:
            print(f"FAIL: LLVM went in as a shared library: {shared}", file=sys.stderr)
            return 1
    return 0


def answer(policy: str, sentence: str) -> tuple[str, str]:
    code = (
        "import jay; v = jay.j(%r); "
        "print(repr(v.tolist() if hasattr(v, 'tolist') else v))" % sentence
    )
    run = subprocess.run(
        [sys.executable, "-c", code],
        env=dict(os.environ, LIBJAY_JIT=policy),
        capture_output=True,
        text=True,
        check=True,
    )
    return run.stdout, run.stderr


def installed() -> int:
    import jay

    backends = jay.jit_backends()
    print(f"libjay {jay.__version__} from {os.path.dirname(jay.__file__)}")
    print(f"code generators: {backends}")
    if backends != ("cranelift", "llvm"):
        print("FAIL: this build should have both Cranelift and LLVM", file=sys.stderr)
        return 1
    for sentence in SENTENCES:
        interpreted, _ = answer("0", sentence)
        compiled, err = answer("llvm", sentence)
        if compiled != interpreted or err:
            print(
                f"FAIL: {sentence}\n  llvm: {compiled[:200]}\n"
                f"  interpreter: {interpreted[:200]}\n  stderr: {err}",
                file=sys.stderr,
            )
            return 1
        print(f"ok, LLVM agrees with the interpreter: {sentence}")
    return 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--contents"]:
        sys.exit(contents(sys.argv[2:]))
    sys.exit(installed())
