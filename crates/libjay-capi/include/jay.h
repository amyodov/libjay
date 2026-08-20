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
  JAY_CHAR = 4  /* uint32_t Unicode codepoints (UTF-32) */
} jay_dtype;

/* A borrowed array descriptor: libjay copies what it needs, so the memory
 * only has to stay valid for the duration of the jay_run call. */
typedef struct {
  jay_dtype dtype;
  int32_t rank;          /* 0 for a scalar */
  const uint64_t *shape; /* rank axis lengths; may be NULL when rank is 0 */
  const void *data;      /* row-major, aligned for dtype; may be NULL when empty */
} jay_value;

/* Sink for a program's output (J `echo`, APL `⎕←`). The text is UTF-8 and is
 * not NUL-terminated: len is authoritative. */
typedef void (*jay_write_fn)(const char *text_utf8, size_t len, void *userdata);

/* Compile source_utf8 in lang ("j" or "apl"); index_origin sets APL's ⎕IO,
 * or -1 for the language default. NULL on failure, with *err set. */
jay_program *jay_compile(const char *source_utf8, const char *lang, int32_t index_origin,
                         jay_error **err);

/* Release a program. NULL is a no-op. */
void jay_program_free(jay_program *program);

/* Number of parameters the program expects, in the order jay_run wants them. */
size_t jay_program_param_count(const jay_program *program);

/* Name of parameter i, borrowed from the program; NULL when out of range. */
const char *jay_program_param_name(const jay_program *program, size_t i);

/* Execute a program: args holds nargs values, one per parameter, in order.
 * write NULL sends output to stdout; out NULL discards the result.
 * Returns 0 on success, nonzero on failure with *err set. */
int jay_run(const jay_program *program, const jay_value *args, size_t nargs, jay_write_fn write,
            void *write_userdata, jay_result **out, jay_error **err);

/* 1 when the program yielded no value (its last sentence was an assignment
 * or ⎕←), 0 otherwise. */
int jay_result_is_empty(const jay_result *result);

/* The result's element type; 0 for an empty result. */
jay_dtype jay_result_dtype(const jay_result *result);

/* The result's rank (0 for a scalar); -1 for an empty result. */
int32_t jay_result_rank(const jay_result *result);

/* The result's axis lengths, borrowed from it; rank entries. */
const uint64_t *jay_result_shape(const jay_result *result);

/* The result's row-major elements, borrowed from it, typed by its dtype. */
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
