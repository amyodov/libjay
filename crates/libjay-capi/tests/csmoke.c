/* A C program that uses libjay the way a host application would: compile
 * once, run with your own arrays, read the result, free everything.
 *
 * Built and run by tests/csmoke.rs; its stdout is the assertion.
 */

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "jay.h"

/* Report and stop: an error handle always has a rendered message. */
static void die(jay_error *err, const char *what) {
  char *msg = jay_error_message(err);
  fprintf(stderr, "%s: %s\n", what, msg ? msg : "(no message)");
  jay_string_free(msg);
  jay_error_free(err);
  exit(1);
}

/* Where a program's `echo` output goes when the host wants it. */
static void echo_to_stdout(const char *text_utf8, size_t len, void *userdata) {
  (void)userdata;
  fwrite(text_utf8, 1, len, stdout);
}

int main(void) {
  jay_error *err = NULL;
  jay_result *res = NULL;

  /* J: the mean of a float vector, as the fork (sum divided by count). */
  jay_program *mean = jay_compile("(+/ % #) {x}", "j", -1, &err);
  if (!mean) die(err, "compile mean");
  assert(jay_program_param_count(mean) == 1);
  assert(strcmp(jay_program_param_name(mean, 0), "x") == 0);

  const double values[] = {1.0, 2.0, 3.0, 4.0};
  const uint64_t shape[] = {4};
  jay_value arg;
  arg.dtype = JAY_F64;
  arg.rank = 1;
  arg.shape = shape;
  arg.data = values;

  if (jay_run(mean, &arg, 1, NULL, NULL, &res, &err) != 0) die(err, "run mean");
  assert(jay_result_is_empty(res) == 0);
  assert(jay_result_dtype(res) == JAY_F64);
  assert(jay_result_rank(res) == 0);
  double got = *(const double *)jay_result_data(res);
  printf("mean=%g\n", got);
  assert(got == 2.5);
  jay_result_free(res);
  jay_program_free(mean);

  /* APL, with its own glyphs written straight into a UTF-8 literal. */
  jay_program *apl = jay_compile("+/⍳5", "apl", -1, &err);
  if (!apl) die(err, "compile apl");
  res = NULL;
  if (jay_run(apl, NULL, 0, NULL, NULL, &res, &err) != 0) die(err, "run apl");
  char *text = jay_result_format(res);
  printf("apl=%s\n", text);
  assert(strcmp(text, "15") == 0);
  jay_string_free(text);
  jay_result_free(res);
  jay_program_free(apl);

  /* Program output is routed by the host, not assumed to be stdout. */
  jay_program *chatty = jay_compile("echo 'hello from j'", "j", -1, &err);
  if (!chatty) die(err, "compile echo");
  printf("echo=");
  if (jay_run(chatty, NULL, 0, echo_to_stdout, NULL, NULL, &err) != 0) die(err, "run echo");
  jay_program_free(chatty);

  /* A failed compile hands back an error that points into the source. */
  jay_program *bad = jay_compile("(1 + 2", "j", -1, &err);
  assert(bad == NULL);
  assert(err != NULL);
  char *msg = jay_error_message(err);
  assert(strchr(msg, '^') != NULL);
  jay_string_free(msg);
  jay_error_free(err);
  printf("caret=ok\n");

  printf("version=%s\n", jay_version());
  return 0;
}
