/* jay.h — stable C ABI for libjay: compile J/APL expressions, run them on
 * your own arrays.
 *
 * Model, as in PCRE: jay_compile() once, jay_run() many times. A program is
 * immutable and holds no data; arguments are passed per run.
 *
 * Ownership: every pointer this library returns is owned by the caller and
 * has exactly one matching free (jay_program_free, jay_result_free,
 * jay_error_free, jay_string_free). Pointers documented as "borrowed" stay
 * valid until the handle they come from is freed. Passing NULL to any
 * function is defined behaviour, never a crash.
 *
 * Threads: a jay_program is immutable and may be run concurrently from any
 * number of threads. A jay_result and a jay_error are not shared.
 *
 * Link with -ljay (libjay.so / libjay.dylib / jay.dll, or libjay.a).
 *
 * MIT licensed. https://github.com/amyodov/libjay
 */

#ifndef LIBJAY_JAY_H
#define LIBJAY_JAY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* A compiled program: immutable, reusable, holds no data bindings. */
typedef struct jay_program jay_program;

/* One value produced by a run. */
typedef struct jay_result jay_result;

/* A failure, with the position in the source it came from. */
typedef struct jay_error jay_error;

/* Element type of an array crossing the boundary. */
typedef enum {
  JAY_BOOL = 1, /* uint8_t, 0 or 1 */
  JAY_I64 = 2,  /* int64_t */
  JAY_F64 = 3,  /* double */
  JAY_CHAR = 4,   /* uint32_t Unicode codepoints (UTF-32) */
  JAY_COMPLEX = 5 /* two doubles per element: real, then imaginary */
} jay_dtype;

/* A borrowed array descriptor: libjay copies what it needs, so the memory
 * only has to stay valid for the duration of the jay_run call. */
typedef struct {
  jay_dtype dtype;
  int32_t rank;          /* 0 for a scalar */
  const uint64_t *shape; /* rank axis lengths; may be NULL when rank is 0 */
  const void *data;      /* row-major, aligned for dtype; may be NULL when empty.
                          * JAY_COMPLEX is double[2] per element, interleaved. */
} jay_value;

/* Sink for a program's output (J `echo`, APL `⎕←`). The text is UTF-8 and is
 * not NUL-terminated: len is authoritative. */
typedef void (*jay_write_fn)(const char *text_utf8, size_t len, void *userdata);

/* Source for a program's input (APL `⍞` and `⎕`, J `1!:1 ]1`): one line per
 * call, UTF-8, with no line terminator and no NUL.
 *
 * The return follows snprintf. 0..cap bytes were written into buf and are
 * the line. A value ABOVE cap means nothing was written and the line needs
 * that many bytes: libjay grows its buffer and calls again, so a source
 * answering this way must not have consumed the line yet. Negative is the
 * end of the input. */
typedef int (*jay_read_fn)(char *buf, size_t cap, void *userdata);

/* Non-standard extensions: opt-in departures from what the reference
 * implementations answer, combined with |. A dialect setting chooses between
 * readings some reference implements; an extension is a reading none of them
 * does, so nothing here is on unless it is asked for. */
typedef uint64_t jay_extensions;
#define JAY_EXT_NONE ((jay_extensions)0)
/* A J quoted literal holds Unicode characters rather than the bytes that
 * spell them, so `# 'e-acute'` is 1 where J answers 2. */
#define JAY_EXT_J_UNICODE_STRINGS ((jay_extensions)1)

/* The bit an extension name spells ("j_unicode_strings", or the environment
 * spelling "LIBJAY_J_UNICODE_STRINGS"); 0 for a name this build has not. */
jay_extensions jay_extension_bit(const char *name);

/* Compile source_utf8 in lang ("j" or "apl"); index_origin sets APL's ⎕IO,
 * or -1 for the language default. The extensions are the process default,
 * which the environment names (LIBJAY_J_UNICODE_STRINGS=1); jay_compile_ext
 * overrides that for one program. NULL on failure, with *err set. */
jay_program *jay_compile(const char *source_utf8, const char *lang, int32_t index_origin,
                         jay_error **err);

/* jay_compile with the extensions named outright rather than taken from the
 * environment, so a library that embeds libjay says what it compiles under.
 * A bit this build does not have is a failure, not a silent no-op. */
jay_program *jay_compile_ext(const char *source_utf8, const char *lang, int32_t index_origin,
                             jay_extensions extensions, jay_error **err);

/* Release a program. NULL is a no-op. */
void jay_program_free(jay_program *program);

/* Number of parameters the program expects, in the order jay_run wants them. */
size_t jay_program_param_count(const jay_program *program);

/* Name of parameter i, borrowed from the program; NULL when out of range. */
const char *jay_program_param_name(const jay_program *program, size_t i);

/* Execute a program: args holds nargs values, one per parameter, in order.
 * write NULL sends output to stdout; out NULL discards the result.
 * Returns 0 on success, nonzero on failure with *err set.
 *
 * The run has no input source: an expression that reads one says so rather
 * than reading anything. Use jay_run_io to attach one. */
int jay_run(const jay_program *program, const jay_value *args, size_t nargs, jay_write_fn write,
            void *write_userdata, jay_result **out, jay_error **err);

/* jay_run with both halves of stdio wired. write NULL sends output to this
 * process's stdout and read NULL takes input from its stdin, which is the
 * sandbox's default on both sides. Everything else is as jay_run. */
int jay_run_io(const jay_program *program, const jay_value *args, size_t nargs, jay_write_fn write,
               void *write_userdata, jay_read_fn read, void *read_userdata, jay_result **out,
               jay_error **err);

/* 1 when the program yielded no value (its last sentence was an assignment
 * or ⎕←), 0 otherwise. */
int jay_result_is_empty(const jay_result *result);

/* The result's element type; 0 for an empty result. */
jay_dtype jay_result_dtype(const jay_result *result);

/* The result's rank (0 for a scalar); -1 for an empty result. */
int32_t jay_result_rank(const jay_result *result);

/* The result's axis lengths, borrowed from it; rank entries. */
const uint64_t *jay_result_shape(const jay_result *result);

/* The result's row-major elements, borrowed from it, typed by its dtype.
 * A JAY_COMPLEX result is double[2] per element, real then imaginary. */
const void *jay_result_data(const jay_result *result);

/* The result formatted the way its language displays it, with no trailing
 * newline; free with jay_string_free. */
char *jay_result_format(const jay_result *result);

/* Release a result. NULL is a no-op. */
void jay_result_free(jay_result *result);

/* The error rendered for display, with the source line and a caret under the
 * offending text; free with jay_string_free. */
char *jay_error_message(const jay_error *err);

/* Release an error. NULL is a no-op. */
void jay_error_free(jay_error *err);

/* Release a string returned by this library. NULL is a no-op. */
void jay_string_free(char *s);

/* The libjay version, as a static string. Never NULL. */
const char *jay_version(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* LIBJAY_JAY_H */
