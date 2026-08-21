# Embedding libjay

libjay is a library first. Rust hosts use the `libjay` crate directly,
Python hosts use the `libjay` package, and everything else goes through the
C ABI described here.

## C ABI

The C surface lives in `crates/libjay-capi`. It builds one shared library
and one static library, both named for `-ljay`:

| Platform | Shared | Static |
|---|---|---|
| Linux | `libjay.so` | `libjay.a` |
| macOS | `libjay.dylib` | `libjay.a` |
| Windows | `jay.dll` | `jay.lib` |

```sh
cargo build -p libjay-capi --release
cc app.c -Icrates/libjay-capi/include -Ltarget/release -ljay -o app
```

Each GitHub release also attaches prebuilt bundles as assets — one
`libjay-capi-<target-triple>.tar.gz` per platform, holding `jay.h` and that
platform's shared and static libraries: `x86_64-unknown-linux-gnu`,
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`.
There is no `aarch64-unknown-linux-gnu` bundle; a linux-aarch64 C caller
builds from source with the command above. The Python wheels cover
linux-aarch64 (and every other platform above) regardless — this gap is C
ABI only.

The header, `crates/libjay-capi/include/jay.h`, is hand-written C99 and is
the contract; every declaration carries a one-line comment.

### Shape

The same PCRE split as the Rust and Python surfaces: compile once, run many
times.

```c
jay_error *err = NULL;
jay_program *p = jay_compile("(+/ % #) {x}", "j", -1, &err);
if (!p) { /* jay_error_message(err), then jay_error_free(err) */ }

const double xs[] = {1, 2, 3, 4};
const uint64_t shape[] = {4};
jay_value arg = {JAY_F64, 1, shape, xs};

jay_result *r = NULL;
if (jay_run(p, &arg, 1, NULL, NULL, &r, &err) != 0) { /* ... */ }
double mean = *(const double *)jay_result_data(r);   /* 2.5 */

jay_result_free(r);
jay_program_free(p);
```

`{name}` holes in the source become parameters. `jay_program_param_count`
and `jay_program_param_name` report them in the order `jay_run` expects
them; arguments are positional.

The third argument to `jay_compile` is APL's `⎕IO`; pass `-1` for the
language default. J's index origin is 0 and is not configurable.

### Data

Values cross as `jay_value`: a dtype tag, a rank, `rank` axis lengths, and a
row-major element buffer. `jay_dtype` has five tags: `JAY_BOOL` (one
`uint8_t`, 0 or 1), `JAY_I64`, `JAY_F64`, `JAY_CHAR` (`uint32_t` Unicode
codepoints, UTF-32 in both directions), and `JAY_COMPLEX` — two `double`s
per element, real then imaginary, the layout of C99's `double _Complex`.
Anything else — a boolean byte that is not 0 or 1, an unknown tag, a NULL
buffer for a non-empty array — is reported as an error rather than guessed
at.

Boxes and J's exact types (extended-precision integers, rationals) have no
`jay_dtype` tag, so a `jay_value` argument can never be one of them. A
program whose result comes out boxed, extended or rational fails `jay_run`
instead of guessing: "boxed results are not in the C ABI yet" for a box,
"extended-precision results are not in the C ABI yet; convert with `_1 x:`
first" and "rational results are not in the C ABI yet; convert with `_1 x:`
first" for the other two — `_1 x:` converts to a machine number inside the
expression, before the result crosses the boundary.

A `jay_value` is borrowed: it only has to stay valid for the duration of the
`jay_run` call. **Input is copied at the boundary today.** The Arrow and
numpy paths are already zero-copy, and the C ABI will grow the same, but
zero-copy ingestion needs the caller to promise a lifetime for its memory
and this ABI does not yet have a way to express that. Correctness first.

Results are read back with `jay_result_dtype` / `_rank` / `_shape` /
`_data`, all borrowed from the result and valid until `jay_result_free`.
`jay_result_format` renders the value the way its language displays it. A
program whose last sentence is an assignment (or `⎕←`) yields no value:
`jay_result_is_empty` returns 1 and the accessors return 0 / -1 / NULL.

### Output

`echo`, `⎕←` and `⍞←` go to the `jay_write_fn` callback passed to `jay_run`,
together with its `userdata`. The text is UTF-8 and is **not**
NUL-terminated — the length argument is authoritative. Passing NULL routes
output to stdout, which is the sandbox default; no other I/O is open.

### Input

The other half of stdio has its own entry point. `jay_run` has no input
source at all: an expression that reads one (APL `⍞` and `⎕`, J `1!:1 ]1`)
reports that rather than reading anything, which is what keeps the
published `jay_run` signature exactly what it was. `jay_run_io` is the same
call with two arguments more:

```c
int rc = jay_run_io(program, args, nargs,
                    write, write_userdata,
                    read, read_userdata,
                    &result, &err);
```

`jay_read_fn` is handed a buffer libjay owns and answers one line per call:

```c
static int read_line(char *buf, size_t cap, void *userdata) {
  FILE *f = (FILE *)userdata;
  static char *line = NULL;
  static size_t n = 0;
  if (line == NULL && getline(&line, &n, f) < 0) return -1;   /* end of input */
  size_t len = strcspn(line, "\n");
  if (len > cap) return (int)len;         /* ask again with a bigger buffer */
  memcpy(buf, line, len);
  free(line); line = NULL; n = 0;
  return (int)len;
}
```

The return follows `snprintf`: `0..cap` bytes were written into `buf` and
are the line, a count **above** `cap` means nothing was written and libjay
should grow its buffer and ask for the same line again, and a negative
value is the end of the input. The text is UTF-8, without a terminator and
without a NUL. A NULL `read` takes input from this process's stdin, as a
NULL `write` sends output to its stdout — the sandbox's default on both
sides. Reading past the end of the input is a reported error, never an
empty line.

### Errors

Failures hand back a `jay_error *`. `jay_error_message` renders it the way
the CLI would, with the source line and a caret under the offending text:

```
length error: arguments do not agree: left shape 3, right shape 2
  1 2 3 + 1 2
  ^^^^^^^^^^^
note: frames first differ at axis 0: 3 vs 2
```

Compile errors point into the source you passed to `jay_compile`; run errors
point into the program's source.

### Rules

- Every pointer the library returns is owned by the caller and has exactly
  one free: `jay_program_free`, `jay_result_free`, `jay_error_free`,
  `jay_string_free`. Never `free()` them yourself.
- Passing NULL to any function is defined: it is a no-op or a documented
  sentinel, never a crash.
- Panics are caught at every boundary and surface as an "internal panic"
  error; unwinding never crosses into C.
- A `jay_program` is immutable and holds no data, so it can be run
  concurrently from any number of threads. A `jay_result` and a `jay_error`
  belong to one thread at a time.

### Building the crate

`crates/libjay-capi` declares `crate-type = ["cdylib", "staticlib"]` and no
`rlib`, deliberately: the core crate's library is also named `jay`, so an
rlib here would produce a second `libjay.rlib` in the shared target
directory and collide with it. Nothing links this crate as a Rust
dependency — its consumers are C — so the rlib has no use. The FFI tests
include `src/lib.rs` by path for the same reason.
